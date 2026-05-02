import type { PlantConfig } from './types';
import { apiBase } from '../config';

export async function fetchPlantConfig(): Promise<PlantConfig> {
  const res = await fetch(`${apiBase()}/api/plant`);
  if (!res.ok) throw new Error(`/api/plant responded ${res.status}`);
  return res.json() as Promise<PlantConfig>;
}
