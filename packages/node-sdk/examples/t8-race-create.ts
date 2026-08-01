// T8.4 driver: create session with explicit id, twice concurrently in same process.
import { createDimiHarness, type DimiHarness } from '@dimi-agent/dimi-sdk';

const workDir = process.argv[2]!;
const homeDir = process.argv[3]!;
const sessionId = process.argv[4]!;

const identity: any = { userAgentProduct: 'dimi-cli', version: '0.0.1-test' };
const harnessA = createDimiHarness({ identity, homeDir });
const harnessB = createDimiHarness({ identity, homeDir });

async function run(label: string, h: DimiHarness): Promise<void> {
  try {
    const s = await h.createSession({ workDir, id: sessionId, model: 'dimi/kimi-for-coding' });
    console.log(JSON.stringify({ label, ok: true, id: s.id, dir: s.summary?.sessionDir }));
  } catch (error: any) {
    console.log(JSON.stringify({ label, ok: false, msg: String(error.message ?? error), code: error.code ?? error.cause?.code }));
  } finally {
    await h.close();
  }
}

await Promise.all([run('A', harnessA), run('B', harnessB)]);
