/// K8s reconciler — keeps simulated PLC pods in sync with the desired state.
///
/// Desired state: simulated PLCs from plant config (or DB when available).
/// Actual state:  Deployments/Services/PVCs labeled `managed-by=factory-sim`.
///
/// Runs `kubectl` as a subprocess so the backend binary has zero k8s crate deps.
/// The pod must have a ServiceAccount with permission to manage Deployments,
/// Services, and PVCs in its namespace (see helm/factory-sim/templates/rbac.yaml).
///
/// Also runs a node-sync loop that updates `deploy_nodes` from live k8s node labels.
use std::collections::HashMap;
use sqlx::PgPool;

const LABEL_MANAGED_BY: &str = "managed-by=factory-sim";
const RECONCILE_INTERVAL_SECS: u64 = 15;
const NODE_SYNC_INTERVAL_SECS: u64 = 30;

/// Start the reconcile + node-sync tasks if `kubectl` is reachable (in-cluster).
/// Silently skips if kubectl is unavailable — harmless outside a cluster.
/// Reads desired PLC state from the DB, not the in-memory plant config.
pub fn start(
    pool:            PgPool,
    simulator_image: String,
    backend_url:     String,
    namespace:       String,
) {
    if !kubectl_available() {
        tracing::info!("[k8s] kubectl not found — skipping reconciler (not in-cluster?)");
        return;
    }
    tracing::info!("[k8s] reconciler starting  namespace={}  image={}", namespace, simulator_image);

    // Reconcile loop
    {
        let pool2   = pool.clone();
        let ns      = namespace.clone();
        let image   = simulator_image.clone();
        let be_url  = backend_url.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(
                tokio::time::Duration::from_secs(RECONCILE_INTERVAL_SECS)
            );
            loop {
                interval.tick().await;
                if let Err(e) = reconcile(&pool2, &image, &be_url, &ns).await {
                    tracing::warn!("[k8s] reconcile error: {}", e);
                }
            }
        });
    }

    // Node sync loop
    {
        let pool2 = pool.clone();
        let ns2   = namespace.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(
                tokio::time::Duration::from_secs(NODE_SYNC_INTERVAL_SECS)
            );
            loop {
                interval.tick().await;
                if let Err(e) = sync_nodes(&ns2, Some(&pool2)).await {
                    tracing::warn!("[k8s] node-sync error: {}", e);
                }
            }
        });
    }
}

// ============================================================================
// Reconcile
// ============================================================================

async fn reconcile(
    pool:        &PgPool,
    image:       &str,
    backend_url: &str,
    namespace:   &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Desired: all simulated PLCs from the DB.
    #[derive(sqlx::FromRow)]
    struct PlcRow {
        plc_id:      Option<String>,
        name:        String,
        deploy_node: Option<String>,
    }

    let plc_rows = sqlx::query_as::<_, PlcRow>(
        "SELECT plc_id, name, deploy_node FROM plcs WHERE kind = 'simulated'"
    ).fetch_all(pool).await?;

    let desired: HashMap<String, DesiredPlc> = plc_rows.into_iter()
        .map(|p| {
            let slug        = p.plc_id.unwrap_or_else(|| slugify(&p.name));
            let deploy_name = plc_deploy_name(&slug);
            (deploy_name.clone(), DesiredPlc {
                plc_id:        slug,
                deploy_name,
                deploy_target: p.deploy_node,
            })
        })
        .collect();

    // Actual: Deployments currently managed by us.
    let actual = list_managed_deployments(namespace).await?;

    // Create missing.
    for (deploy_name, d) in &desired {
        if !actual.contains(deploy_name) {
            tracing::info!("[k8s] creating sim pod: {}", deploy_name);
            apply_sim_resources(d, image, backend_url, namespace).await?;
        }
    }

    // Delete stale.
    for deploy_name in &actual {
        if !desired.contains_key(deploy_name) {
            tracing::info!("[k8s] deleting stale sim pod: {}", deploy_name);
            delete_sim_resources(deploy_name, namespace).await?;
        }
    }

    Ok(())
}

struct DesiredPlc {
    plc_id:        String,
    deploy_name:   String,
    deploy_target: Option<String>,
}

fn plc_deploy_name(plc_id: &str) -> String {
    plc_id.to_lowercase().replace('_', "-")
}

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

async fn list_managed_deployments(namespace: &str) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    let out = kubectl(&[
        "get", "deployments",
        "-n", namespace,
        "-l", LABEL_MANAGED_BY,
        "-o", "jsonpath={.items[*].metadata.name}",
    ]).await?;

    Ok(out.split_whitespace().map(str::to_string).collect())
}

async fn apply_sim_resources(
    d:           &DesiredPlc,
    image:       &str,
    backend_url: &str,
    namespace:   &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let node_selector = d.deploy_target.as_deref()
        .map(|t| format!("deploy_target: {}", t))
        .unwrap_or_default();
    let node_selector_yaml = if node_selector.is_empty() {
        String::new()
    } else {
        format!("      nodeSelector:\n        {}\n", node_selector)
    };

    let yaml = format!(r#"---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {deploy_name}
  namespace: {namespace}
  labels:
    app: {deploy_name}
    managed-by: factory-sim
spec:
  replicas: 1
  selector:
    matchLabels:
      app: {deploy_name}
  template:
    metadata:
      labels:
        app: {deploy_name}
    spec:
{node_selector_yaml}      containers:
      - name: simulator
        image: {image}
        imagePullPolicy: Always
        env:
        - name: SIM_PLC_ID
          value: "{plc_id}"
        - name: BACKEND_URL
          value: "{backend_url}"
        - name: SIM_TICK_MS
          value: "100"
        - name: OPCUA_HOST
          value: {deploy_name}
        - name: PKI_DIR
          value: /data/pki
        ports:
        - containerPort: 4840
        - containerPort: 9000
          name: health
        livenessProbe:
          httpGet:
            path: /health
            port: 9000
          initialDelaySeconds: 15
          periodSeconds: 15
          failureThreshold: 3
        volumeMounts:
        - name: data
          mountPath: /data
      volumes:
      - name: data
        persistentVolumeClaim:
          claimName: {deploy_name}-data
---
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: {deploy_name}-data
  namespace: {namespace}
  labels:
    app: {deploy_name}
    managed-by: factory-sim
spec:
  accessModes:
  - ReadWriteOnce
  resources:
    requests:
      storage: 100Mi
---
apiVersion: v1
kind: Service
metadata:
  name: {deploy_name}
  namespace: {namespace}
  labels:
    app: {deploy_name}
    managed-by: factory-sim
spec:
  type: ClusterIP
  selector:
    app: {deploy_name}
  ports:
  - port: 4840
    targetPort: 4840
"#,
        deploy_name   = d.deploy_name,
        namespace     = namespace,
        plc_id        = d.plc_id,
        image         = image,
        backend_url   = backend_url,
        node_selector_yaml = node_selector_yaml,
    );

    kubectl_apply(&yaml).await
}

async fn delete_sim_resources(
    deploy_name: &str,
    namespace:   &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Delete Deployment — k8s garbage-collects its ReplicaSet+Pods automatically.
    kubectl(&["delete", "deployment", deploy_name, "-n", namespace, "--ignore-not-found"]).await?;
    kubectl(&["delete", "service",    deploy_name, "-n", namespace, "--ignore-not-found"]).await?;
    kubectl(&["delete", "pvc", &format!("{}-data", deploy_name), "-n", namespace, "--ignore-not-found"]).await?;
    Ok(())
}

// ============================================================================
// Node sync — updates deploy_nodes from live k8s node labels
// ============================================================================

async fn sync_nodes(
    _namespace: &str,
    db: Option<&PgPool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let Some(pool) = db else { return Ok(()); };

    // List all nodes, extract deploy_target label + arch + allocatable memory.
    let json_str = kubectl(&[
        "get", "nodes",
        "-o", r#"jsonpath={range .items[*]}{.metadata.name}{"\t"}{.metadata.labels.deploy_target}{"\t"}{.status.nodeInfo.architecture}{"\t"}{.status.allocatable.memory}{"\t"}{range .status.conditions[?(@.type=="Ready")]}{.status}{end}{"\n"}{end}"#,
    ]).await?;

    for line in json_str.lines() {
        let parts: Vec<&str> = line.splitn(5, '\t').collect();
        if parts.len() < 5 { continue; }
        let (k8s_node, deploy_target, arch, mem_str, ready_str) = (
            parts[0], parts[1], parts[2], parts[3], parts[4].trim()
        );
        if deploy_target.is_empty() { continue; }

        let mem_mb = parse_mem_to_mb(mem_str);
        let status = if ready_str == "True" { "ready" } else { "notready" };

        sqlx::query(
            "INSERT INTO deploy_nodes (name, display_name, k8s_node, arch, mem_allocatable_mb, status, last_seen)
             VALUES ($1, $1, $2, $3, $4, $5, NOW())
             ON CONFLICT (name) DO UPDATE SET
               k8s_node = EXCLUDED.k8s_node,
               arch = EXCLUDED.arch,
               mem_allocatable_mb = EXCLUDED.mem_allocatable_mb,
               status = EXCLUDED.status,
               last_seen = EXCLUDED.last_seen"
        )
        .bind(deploy_target)
        .bind(k8s_node)
        .bind(arch)
        .bind(mem_mb)
        .bind(status)
        .execute(pool)
        .await?;

        tracing::debug!("[k8s] node-sync: {} → {} ({}  {}MB  {})", deploy_target, k8s_node, arch, mem_mb.unwrap_or(0), status);
    }

    Ok(())
}

fn parse_mem_to_mb(s: &str) -> Option<i32> {
    if s.ends_with("Ki") {
        s[..s.len()-2].parse::<i64>().ok().map(|v| (v / 1024) as i32)
    } else if s.ends_with("Mi") {
        s[..s.len()-2].parse::<i32>().ok()
    } else if s.ends_with("Gi") {
        s[..s.len()-2].parse::<i64>().ok().map(|v| (v * 1024) as i32)
    } else {
        s.parse::<i64>().ok().map(|v| (v / 1_048_576) as i32)
    }
}

// ============================================================================
// kubectl subprocess helpers
// ============================================================================

fn kubectl_available() -> bool {
    std::process::Command::new("kubectl")
        .arg("version")
        .arg("--client")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn kubectl(args: &[&str]) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let output = tokio::process::Command::new("kubectl")
        .args(args)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("kubectl {}: {}", args.join(" "), stderr.trim()).into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn kubectl_apply(yaml: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;

    let mut child = Command::new("kubectl")
        .args(["apply", "-f", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(yaml.as_bytes()).await?;
    }

    let out = child.wait_with_output().await?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("kubectl apply: {}", stderr.trim()).into());
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        tracing::info!("[k8s] {}", line);
    }
    Ok(())
}
