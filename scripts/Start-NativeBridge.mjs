import { randomBytes } from 'node:crypto';
import { execFile, spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { access } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { promisify } from 'node:util';

const project = dirname(dirname(fileURLToPath(import.meta.url)));
// A release package keeps the patched Codex in bin\ (the official package layout); the
// repository keeps it in native\.
const packaged = join(project, 'bin', 'codex.exe');
const binary = existsSync(packaged) ? packaged : join(project, 'native', 'codex.exe');
const bridge = process.env.MAGICODEX_BRIDGE_ROOT || join(homedir(), '.codex', 'copilot-proxy');
const load = name => import(pathToFileURL(join(bridge, 'src', name)).href);
const { isInformationRequest, validateCliArgs } = await load('arguments.mjs');
const { createClient, readSettings, stopClient } = await load('runtime.mjs');
const { CopilotBackend, safeErrorMessage } = await load('backend.mjs');
const { buildCatalog, writeLaunchCatalog } = await load('catalog.mjs');
const { codexExecutable, readBundledCatalog } = await load('codex.mjs');
const { startServer } = await load('server.mjs');

const args = process.argv.slice(2);
validateCliArgs(args);
await access(binary);

async function waitForExit(child) {
  return new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('exit', (code, signal) => resolve(code ?? (signal ? 130 : 1)));
  });
}

if (isInformationRequest(args)) {
  process.exitCode = await waitForExit(spawn(binary, args, { stdio: 'inherit' }));
} else {
  const settings = await readSettings();
  const client = createClient(settings);
  let backend;
  let server;
  let catalog;
  let child;
  let terminated = false;
  const interrupt = () => {
    if (!child) {
      terminated = true;
      void client.forceStop().catch(error => console.error(safeErrorMessage(error)));
    }
  };
  const terminate = () => {
    terminated = true;
    if (child && child.exitCode === null) child.kill('SIGTERM');
    else void client.forceStop().catch(error => console.error(safeErrorMessage(error)));
  };
  process.on('SIGINT', interrupt);
  process.on('SIGTERM', terminate);
  try {
    const { stdout } = await promisify(execFile)(codexExecutable(settings), ['--version'], {
      timeout: 10000, windowsHide: true,
    });
    if (stdout.trim() !== 'codex-cli 0.153.4') {
      throw new Error('This native patch targets Codex 0.153.4. Rebuild the matching patch before using a different installed version.');
    }
    await client.start();
    if (!(await client.getAuthStatus()).isAuthenticated) {
      throw new Error('Sign in to the existing GitHub Copilot CLI first. No other account was selected.');
    }
    const models = await client.listModels();
    backend = new CopilotBackend(client, models, settings);
    catalog = await writeLaunchCatalog(buildCatalog(
      models, await readBundledCatalog(codexExecutable(settings)), settings.defaultModel,
    ));
    const token = randomBytes(32).toString('hex');
    server = await startServer(backend, token);
    if (terminated) {
      process.exitCode = 130;
    } else {
      const provider = [
        'name="GitHub Copilot local bridge"',
        `base_url=${JSON.stringify(server.baseUrl)}`,
        'wire_api="responses"',
        'env_key="CODEX_COPILOT_PROXY_TOKEN"',
        'requires_openai_auth=false',
        'supports_websockets=false',
        'request_max_retries=0',
        'stream_max_retries=0',
        `stream_idle_timeout_ms=${settings.requestTimeoutMs}`,
      ].join(',');
      if (settings.profileName) {
        await access(join(process.env.CODEX_HOME || join(homedir(), '.codex'), `${settings.profileName}.config.toml`));
      }
      const providerConfig = settings.profileName
        ? `model_providers.copilot_bridge.base_url=${JSON.stringify(server.baseUrl)}`
        : `model_providers.copilot_bridge={${provider}}`;
      const noProxy = [...new Set([
        ...(process.env.NO_PROXY ?? process.env.no_proxy ?? '').split(',').filter(Boolean),
        '127.0.0.1', 'localhost', '::1',
      ])].join(',');
      child = spawn(binary, [
        ...(settings.profileName ? ['--profile', settings.profileName] : []),
        ...(!settings.profileName ? ['-c', `model=${JSON.stringify(settings.defaultModel)}`] : []),
        '-c', `model_catalog_json=${JSON.stringify(catalog.path)}`,
        '-c', 'model_provider="copilot_bridge"',
        '-c', providerConfig,
        '-c', `model_providers.copilot_bridge.stream_idle_timeout_ms=${settings.requestTimeoutMs}`,
        '-c', 'web_search="disabled"',
        '-c', 'features.multi_agent=false',
        '-c', 'check_for_update_on_startup=false',
        ...args,
      ], {
        cwd: process.cwd(),
        env: {
          ...process.env,
          CODEX_COPILOT_PROXY_TOKEN: token,
          CODEX_MANAGED_PACKAGE_ROOT: dirname(dirname(settings.codexEntry)),
          CODEX_MANAGED_BY_NPM: '1',
          NO_PROXY: noProxy,
          no_proxy: noProxy,
        },
        stdio: 'inherit',
      });
      process.exitCode = await waitForExit(child);
    }
  } catch (error) {
    console.error(`Magicodex native bridge: ${safeErrorMessage(error)}`);
    process.exitCode = 1;
  } finally {
    process.removeListener('SIGINT', interrupt);
    process.removeListener('SIGTERM', terminate);
    const results = await Promise.allSettled([
      ...(server ? [server.close()] : []),
      ...(backend ? [backend.close()] : []),
      ...(catalog ? [catalog.dispose()] : []),
    ]);
    try { await stopClient(client); }
    catch (error) { results.push({ status: 'rejected', reason: error }); }
    for (const result of results) {
      if (result.status === 'rejected') {
        console.error(`Native bridge cleanup: ${safeErrorMessage(result.reason)}`);
        process.exitCode = 1;
      }
    }
  }
}
