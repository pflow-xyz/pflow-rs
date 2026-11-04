// compose_canvas: the snap-together composition page (crates/pflow-wasm/compose),
// written out as one self-contained HTML file with the engine embedded: no server,
// no network, so it can be published as a Claude artifact.
import { copyFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { z } from 'zod';

export function register(tool) {
  tool('compose_canvas',
    'Write the snap-together composition canvas as one self-contained HTML file (about 1 MB) and return its path; publish that file as an artifact. In the page: add parts from a 13-part catalog, drag from a port to another (the ports it can legally join light up), and read in plain language what each join does (laws kept, explained or broken, boundedness, dead ends, with a proof record the page re-checks twice: by the engine and by a separate arithmetic-and-replay script). The composite also plays as an ordinary net. Everything runs in the browser on the embedded wasm engine.',
    { path: z.string().optional().describe('Where to write it (default: a temp file)') },
    ({ path }) => {
      const file = path ?? join(tmpdir(), 'pflow-compose-canvas.html');
      copyFileSync(new URL('./compose-canvas.html', import.meta.url), file);
      return { path: file };
    });
}
