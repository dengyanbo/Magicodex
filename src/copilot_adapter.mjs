import { randomBytes } from 'node:crypto';
import { spawn } from 'node:child_process';
import { dirname, join } from 'node:path';
import { pathToFileURL } from 'node:url';

// Reuse the installed bridge; this adapter changes neither its files nor its account.
const root = process.env.MAGICODEX_BRIDGE_ROOT;
const overrides = JSON.parse(process.env.MAGICODEX_PROFILE_OVERRIDES);
delete process.env.MAGICODEX_BRIDGE_ROOT;
delete process.env.MAGICODEX_PROFILE_OVERRIDES;
const load = name => import(pathToFileURL(join(root, 'src', name)).href);
const { createClient, readSettings, stopClient } = await load('runtime.mjs');
const { CopilotBackend, safeErrorMessage } = await load('backend.mjs');
const { buildCatalog, writeLaunchCatalog } = await load('catalog.mjs');
const { codexExecutable, readBundledCatalog } = await load('codex.mjs');
const { startServer } = await load('server.mjs');
const { parseRequest } = await load('protocol.mjs');

const settings = await readSettings();
const binary = codexExecutable(settings);
const client = createClient(settings);
let backend;
let server;
let catalog;
let child;
let failure;
try {
  await client.start();
  if (!(await client.getAuthStatus()).isAuthenticated) {
    throw new Error('The selected Copilot bridge is not signed in. No alternate account was selected.');
  }
  const models = await client.listModels();
  backend = new CopilotBackend(client, models, settings);
  catalog = await writeLaunchCatalog(buildCatalog(models, await readBundledCatalog(binary), settings.defaultModel));
  const token = randomBytes(32).toString('hex');
  let previousTools;
  const transport = {
    listModels: () => backend.listModels(),
    async respond(body, headers, emit, signal) {
      if (process.env.MAGICODEX_BRIDGE_DIAGNOSTICS === '1') {
        const tools = parseRequest({
          ...body,
          input: Array.isArray(body.input) ? body.input.filter(item => item?.type === 'additional_tools') : [],
        }).definitions;
        if (previousTools && JSON.stringify(previousTools) !== JSON.stringify(tools)) {
          const paths = [];
          const compare = (a, b, path) => {
            if (paths.length >= 32 || JSON.stringify(a) === JSON.stringify(b)) return;
            if (a && b && typeof a === 'object' && typeof b === 'object') {
              for (const key of new Set([...Object.keys(a), ...Object.keys(b)])) {
                compare(a[key], b[key], `${path}.${key}`);
              }
            } else { paths.push(path); }
          };
          compare(previousTools, tools, 'tools');
          console.error(`Magicodex tool-catalog changed fields: ${paths.join(', ')}`);
          console.error(`Magicodex tools before: ${previousTools.map(t => `${t.namespace ?? ''}.${t.name}`).join(', ')}`);
          console.error(`Magicodex tools after: ${tools.map(t => `${t.namespace ?? ''}.${t.name}`).join(', ')}`);
        }
        previousTools = structuredClone(tools);
      }
      return backend.respond(body, headers, emit, signal);
    },
  };
  server = await startServer(transport, token);
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
  const profileArgs = overrides.flatMap(value => ['-c', value]);
  const dynamicProvider = settings.profileName
    ? `model_providers.copilot_bridge.base_url=${JSON.stringify(server.baseUrl)}`
    : `model_providers.copilot_bridge={${provider}}`;
  const noProxy = [...new Set([
    ...(process.env.NO_PROXY ?? process.env.no_proxy ?? '').split(',').filter(Boolean),
    '127.0.0.1', 'localhost', '::1',
  ])].join(',');
  child = spawn(binary, [
    ...profileArgs,
    ...(!settings.profileName ? ['-c', `model=${JSON.stringify(settings.defaultModel)}`] : []),
    '-c', `model_catalog_json=${JSON.stringify(catalog.path)}`,
    '-c', 'model_provider="copilot_bridge"',
    '-c', dynamicProvider,
    '-c', `model_providers.copilot_bridge.stream_idle_timeout_ms=${settings.requestTimeoutMs}`,
    '-c', 'web_search="disabled"',
    '-c', 'features.multi_agent=false',
    'app-server', '--stdio',
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
    windowsHide: true,
  });
  process.exitCode = await new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('exit', (code, signal) => resolve(code ?? (signal ? 130 : 1)));
  });
} catch (error) {
  failure = error;
  process.exitCode = 1;
} finally {
  const results = await Promise.allSettled([
    ...(server ? [server.close()] : []),
    ...(backend ? [backend.close()] : []),
    ...(catalog ? [catalog.dispose()] : []),
  ]);
  try { await stopClient(client); }
  catch (error) { results.push({ status: 'rejected', reason: error }); }
  for (const result of results) {
    if (result.status === 'rejected') {
      process.exitCode = 1;
      console.error(`Magicodex bridge cleanup: ${safeErrorMessage(result.reason)}`);
    }
  }
}
if (failure) console.error(`Magicodex bridge: ${safeErrorMessage(failure)}`);
