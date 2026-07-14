import { randomBytes } from "node:crypto";
import { chmod, mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { readGlobalConfig } from "../engine/config.js";
import { defaultHome } from "../engine/utils/path.js";
import { DaemonError } from "./errors.js";


export const DEFAULT_SERVER_HOST = "127.0.0.1";
export const DEFAULT_SERVER_PORT = 7_999;

export type ServerListenAddress = {
  host: string;
  port: number;
};


export function daemonHome(home?: string): string {
  return join(home ?? defaultHome(), "daemon");
}


export function daemonTokenPath(home?: string): string {
  return join(daemonHome(home), "token");
}


export function configuredListenAddress(listen?: string): ServerListenAddress {
  if (listen) return parseListenAddress(listen);
  const configured = readGlobalConfig().server;
  return parseListenAddress(`${configured?.host ?? DEFAULT_SERVER_HOST}:${configured?.port ?? DEFAULT_SERVER_PORT}`);
}


export function configuredServerUrl(): string {
  const config = readGlobalConfig();
  if (config.client?.serverUrl) return config.client.serverUrl;
  const listen = configuredListenAddress();
  const host = listen.host.includes(":") ? `[${listen.host}]` : listen.host;
  return `http://${host}:${listen.port}/mcp`;
}


export function parseListenAddress(value?: string): ServerListenAddress {
  const listen = value ?? `${DEFAULT_SERVER_HOST}:${DEFAULT_SERVER_PORT}`;
  const separator = listen.lastIndexOf(":");
  if (separator <= 0 || separator === listen.length - 1) {
    throw new DaemonError("INVALID_LISTEN_ADDRESS", "listen must use host:port format.");
  }
  const host = listen.slice(0, separator).replace(/^\[|\]$/g, "");
  const port = Number(listen.slice(separator + 1));
  if (!isLoopbackHost(host)) {
    throw new DaemonError("LOOPBACK_REQUIRED", "Server MVP only supports loopback listen addresses.");
  }
  if (!Number.isInteger(port) || port < 1 || port > 65_535) {
    throw new DaemonError("INVALID_LISTEN_ADDRESS", "listen port must be between 1 and 65535.");
  }
  return { host, port };
}


export function isLoopbackHost(host: string): boolean {
  const normalized = host.toLowerCase();
  return normalized === "127.0.0.1" || normalized === "::1" || normalized === "localhost";
}


export async function resolveServerToken(options: {
  token?: string;
  tokenFile?: string;
  home?: string;
} = {}): Promise<{ token: string; tokenFile?: string }> {
  const explicit = options.token ?? process.env.ZVEC_GREP_SERVER_TOKEN;
  if (explicit) {
    validateToken(explicit);
    return { token: explicit };
  }

  const tokenFile = options.tokenFile
    ?? process.env.ZVEC_GREP_SERVER_TOKEN_FILE
    ?? daemonTokenPath(options.home);
  try {
    const existing = (await readFile(tokenFile, "utf8")).trim();
    validateToken(existing);
    await chmod(tokenFile, 0o600);
    return { token: existing, tokenFile };
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") {
      throw error;
    }
  }

  const token = randomBytes(32).toString("base64url");
  await mkdir(dirname(tokenFile), { recursive: true, mode: 0o700 });
  await writeFile(tokenFile, `${token}\n`, { mode: 0o600, flag: "wx" });
  await chmod(tokenFile, 0o600);
  return { token, tokenFile };
}


function validateToken(token: string): void {
  if (token.length < 32) {
    throw new DaemonError("INVALID_TOKEN", "Server token must contain at least 32 characters.");
  }
}
