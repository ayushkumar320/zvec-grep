import type { CreateZvecGrepOptions } from "../engine/service/types.js";
import { DaemonBackend } from "./backend.js";
import { parseListenAddress, resolveServerToken } from "./config.js";
import { DaemonHttpServer } from "./http-server.js";


export type RunDaemonOptions = {
  version: string;
  listen?: string;
  token?: string;
  tokenFile?: string;
  home?: string;
  serviceOptions?: CreateZvecGrepOptions;
};


export async function runDaemonForeground(options: RunDaemonOptions): Promise<void> {
  const listen = parseListenAddress(options.listen);
  const auth = await resolveServerToken({
    token: options.token,
    tokenFile: options.tokenFile,
    home: options.home,
  });
  const backend = new DaemonBackend({
    version: options.version,
    serviceOptions: options.serviceOptions,
  });
  const httpServer = new DaemonHttpServer({
    ...listen,
    token: auth.token,
    version: options.version,
    backend,
  });
  let address;
  try {
    address = await httpServer.start();
  } catch (error) {
    await backend.close();
    throw error;
  }
  console.log(`zvec-grep server listening on http://${displayHost(address.address)}:${address.port}/mcp`);
  if (auth.tokenFile) {
    console.log(`Bearer token file: ${auth.tokenFile}`);
  }

  await new Promise<void>((resolve) => {
    let stopping = false;
    const stop = () => {
      if (stopping) {
        return;
      }
      stopping = true;
      void (async () => {
        await httpServer.close();
        await backend.close();
      })().finally(resolve);
    };
    process.once("SIGINT", stop);
    process.once("SIGTERM", stop);
  });
}


function displayHost(host: string): string {
  return host.includes(":") ? `[${host}]` : host;
}
