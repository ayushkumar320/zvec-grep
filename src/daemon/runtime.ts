import type { CreateZvecGrepOptions } from "../engine/service/types.js";
import { DaemonBackend } from "./backend.js";
import { configuredListenAddress, resolveServerToken } from "./config.js";
import { DaemonHttpServer } from "./http-server.js";
import { DaemonInstanceLock } from "./server-controller.js";


export type RunDaemonOptions = {
  version: string;
  listen?: string;
  token?: string;
  tokenFile?: string;
  home?: string;
  serviceOptions?: CreateZvecGrepOptions;
};


export async function runDaemonForeground(options: RunDaemonOptions): Promise<void> {
  const listen = configuredListenAddress(options.listen);
  const displayAddress = `http://${displayHost(listen.host)}:${listen.port}/mcp`;
  const instanceLock = await DaemonInstanceLock.acquire(options.home, displayAddress);
  let auth;
  try {
    auth = await resolveServerToken({
      token: options.token,
      tokenFile: options.tokenFile,
      home: options.home,
    });
  } catch (error) {
    await instanceLock.release();
    throw error;
  }
  const backend = new DaemonBackend({ version: options.version, serviceOptions: options.serviceOptions });
  let requestStop: (() => void) | undefined;
  const httpServer = new DaemonHttpServer({
    ...listen, token: auth.token, version: options.version, backend,
    onShutdown: () => requestStop?.(),
  });
  let address;
  try {
    address = await httpServer.start();
  } catch (error) {
    await backend.close();
    await instanceLock.release();
    throw error;
  }
  await instanceLock.markReady();
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
        await instanceLock.release();
      })().finally(resolve);
    };
    requestStop = stop;
    process.once("SIGINT", stop);
    process.once("SIGTERM", stop);
  });
}


function displayHost(host: string): string {
  return host.includes(":") ? `[${host}]` : host;
}
