# @dimi-agent/acp-adapter

Agent Client Protocol adapter for dimi. Exposes the dimi agent over the [Agent Client Protocol](https://agentclientprotocol.com/) so that ACP-compatible clients (editors, IDEs, custom front-ends) can drive a dimi session over stdio.

Part of the [Dimi](https://github.com/zzj3720/dimi) monorepo.

## Minimum usage

```ts
import { createDimiHarness } from '@dimi-agent/dimi-sdk';
import { runAcpServer } from '@dimi-agent/acp-adapter';

const harness = await createDimiHarness();
await runAcpServer(harness);
```

`runAcpServer` reads JSON-RPC from `process.stdin`, writes to `process.stdout`, and resolves when the client closes the connection. SIGINT and SIGTERM trigger a graceful drain that calls `harness.close()` before the process exits.

See `docs/zh/reference/dimi-acp.md` for the full capability matrix (which `Agent` methods are wired, which extensions are stubbed, image / MCP support) and `docs/zh/guides/ides.md` for Zed and JetBrains setup.

## License

MIT
