import { mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { dirname } from "node:path";


export async function readJsonFile<T>(path: string, fallback: T): Promise<T> {
  try {
    const text = await readFile(path, "utf8");
    return JSON.parse(text) as T;
  } catch (error) {
    if (isNodeError(error) && error.code === "ENOENT") {
      return fallback;
    }

    throw error;
  }
}


export function readJsonFileSync<T>(path: string, fallback: T): T {
  try {
    const text = readFileSync(path, "utf8");
    return JSON.parse(text) as T;
  } catch (error) {
    if (isNodeError(error) && error.code === "ENOENT") {
      return fallback;
    }

    throw error;
  }
}


export async function writeJsonFile(path: string, value: unknown): Promise<void> {
  await mkdir(dirname(path), { recursive: true });

  const tmpPath = `${path}.tmp`;
  const text = `${JSON.stringify(value, null, 2)}\n`;

  await writeFile(tmpPath, text, "utf8");
  await rename(tmpPath, path);
}


export function writeJsonFileSync(path: string, value: unknown): void {
  mkdirSync(dirname(path), { recursive: true });

  const tmpPath = `${path}.tmp`;
  const text = `${JSON.stringify(value, null, 2)}\n`;

  writeFileSync(tmpPath, text, "utf8");
  renameSync(tmpPath, path);
}


function isNodeError(error: unknown): error is NodeJS.ErrnoException {
  return typeof error === "object" && error !== null && "code" in error;
}
