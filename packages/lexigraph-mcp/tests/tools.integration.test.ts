// End-to-end integration tests: spin up a real MCP server, connect a real Client over an
// in-memory transport pair, and exercise every Phase-4 tool.

import { mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { createServer } from '../src/server.js';

interface Harness {
  client: Client;
  root: string;
  cleanup(): Promise<void>;
}

async function setup(files: Record<string, string>): Promise<Harness> {
  const root = await mkdtemp(join(tmpdir(), 'lexigraph-mcp-'));
  for (const [rel, content] of Object.entries(files)) {
    await writeFile(join(root, rel), content, 'utf8');
  }

  const { server } = createServer({ root });
  const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
  await server.connect(serverTransport);

  const client = new Client({ name: 'test-client', version: '0.0.0' });
  await client.connect(clientTransport);

  return {
    client,
    root,
    async cleanup() {
      await client.close();
      await server.close();
      await rm(root, { recursive: true, force: true });
    },
  };
}

interface Envelope {
  status: 'ok' | 'error' | 'conflict';
  version?: number;
  data?: unknown;
  message?: string;
  details?: unknown;
  currentVersion?: number;
}

async function call(client: Client, name: string, args: unknown): Promise<Envelope> {
  const result = await client.callTool({ name, arguments: args as Record<string, unknown> });
  const content = result.content as Array<{ type: string; text: string }>;
  expect(content).toHaveLength(1);
  expect(content[0]!.type).toBe('text');
  return JSON.parse(content[0]!.text) as Envelope;
}

describe('lexigraph-mcp tools', () => {
  let h: Harness;
  afterEach(async () => {
    if (h) await h.cleanup();
  });

  describe('workspace_info', () => {
    beforeEach(async () => {
      h = await setup({});
    });

    it('lists supported languages and an empty tracked set', async () => {
      const env = await call(h.client, 'workspace_info', {});
      expect(env.status).toBe('ok');
      const data = env.data as {
        root: string;
        languages: Array<{ id: string }>;
        tracked: unknown[];
      };
      expect(data.root).toBe(h.root);
      expect(data.languages.map((l) => l.id)).toContain('json');
      expect(data.tracked).toEqual([]);
    });
  });

  describe('track_file + read tools', () => {
    beforeEach(async () => {
      h = await setup({ 'a.json': '{"hello": "world"}' });
    });

    it('tracks, surfaces tree summary, and resolves a node by pointer', async () => {
      const tracked = await call(h.client, 'track_file', { path: 'a.json' });
      expect(tracked.status).toBe('ok');
      expect(tracked.version).toBe(0);

      const tree = await call(h.client, 'get_tree', { path: 'a.json', maxDepth: 2 });
      expect(tree.status).toBe('ok');
      const treeData = tree.data as { root: { type: string; children?: unknown[] } };
      expect(treeData.root.type).toBe('document');
      expect(treeData.root.children?.length ?? 0).toBeGreaterThan(0);

      const node = await call(h.client, 'get_node', { path: 'a.json', pointer: '' });
      expect(node.status).toBe('ok');
      const nodeData = node.data as { type: string; text: string };
      expect(nodeData.text).toBe('{"hello": "world"}');
    });

    it('rejects path traversal', async () => {
      const env = await call(h.client, 'track_file', { path: '../escape.json' });
      expect(env.status).toBe('error');
      expect(env.message).toMatch(/escapes workspace/);
    });

    it('errors when reading an untracked file', async () => {
      const env = await call(h.client, 'get_node', { path: 'a.json', pointer: '' });
      expect(env.status).toBe('error');
      expect(env.message).toMatch(/not tracked/);
    });
  });

  describe('query_file', () => {
    beforeEach(async () => {
      h = await setup({ 'q.json': '{"a": 1, "b": 2}' });
    });

    it('returns matches with captures', async () => {
      await call(h.client, 'track_file', { path: 'q.json' });
      const env = await call(h.client, 'query_file', {
        path: 'q.json',
        query: '(pair key: (string) @k)',
      });
      expect(env.status).toBe('ok');
      const data = env.data as {
        matches: Array<{ captures: Array<{ name: string; text: string }> }>;
      };
      expect(data.matches).toHaveLength(2);
      expect(data.matches[0]!.captures[0]!.name).toBe('k');
    });

    it('reports compile errors', async () => {
      await call(h.client, 'track_file', { path: 'q.json' });
      const env = await call(h.client, 'query_file', { path: 'q.json', query: '(((' });
      expect(env.status).toBe('error');
      expect(env.message).toMatch(/query compile/);
    });
  });

  describe('apply_patch + save_file', () => {
    beforeEach(async () => {
      h = await setup({ 'p.json': '{"x": 1}' });
    });

    it('replaces a value, bumps version, and persists to disk', async () => {
      await call(h.client, 'track_file', { path: 'p.json' });
      const node = await call(h.client, 'get_node', { path: 'p.json', pointer: '/children/0' });
      const root = node.data as { children: Array<{ pointer: string; type: string }> };
      const pair = root.children.find((c) => c.type === 'pair')!;
      const pairNode = await call(h.client, 'get_node', { path: 'p.json', pointer: pair.pointer });
      const pairChildren = (pairNode.data as { children: Array<{ pointer: string; type: string }> })
        .children;
      const valuePointer = pairChildren[pairChildren.length - 1]!.pointer;

      const patched = await call(h.client, 'apply_patch', {
        path: 'p.json',
        expectedVersion: 0,
        ops: [{ op: 'replace', path: valuePointer, value: '42' }],
      });
      expect(patched.status).toBe('ok');
      expect(patched.version).toBe(1);

      const conflict = await call(h.client, 'apply_patch', {
        path: 'p.json',
        expectedVersion: 0,
        ops: [{ op: 'replace', path: valuePointer, value: '99' }],
      });
      expect(conflict.status).toBe('conflict');
      expect(conflict.currentVersion).toBe(1);

      const info = await call(h.client, 'workspace_info', {});
      const tracked = (info.data as { tracked: Array<{ dirty: boolean }> }).tracked;
      expect(tracked[0]!.dirty).toBe(true);

      const saved = await call(h.client, 'save_file', { path: 'p.json' });
      expect(saved.status).toBe('ok');
      const onDisk = await readFile(join(h.root, 'p.json'), 'utf8');
      expect(onDisk).toBe('{"x": 42}');
    });

    it('rejects grammar-breaking edits with diagnostics', async () => {
      await call(h.client, 'track_file', { path: 'p.json' });
      const env = await call(h.client, 'apply_patch', {
        path: 'p.json',
        ops: [{ op: 'replace', path: '/children/0', value: '{ broken' }],
      });
      expect(env.status).toBe('error');
      expect(env.message).toMatch(/grammar/);
    });
  });

  describe('diagnostics + untrack_file', () => {
    beforeEach(async () => {
      h = await setup({ 'd.json': '{"ok": 1}' });
    });

    it('reports a clean tree and removes via untrack', async () => {
      await call(h.client, 'track_file', { path: 'd.json' });
      const env = await call(h.client, 'diagnostics', { path: 'd.json' });
      expect(env.status).toBe('ok');
      const data = env.data as { hasError: boolean; diagnostics: unknown[] };
      expect(data.hasError).toBe(false);
      expect(data.diagnostics).toEqual([]);

      const removed = await call(h.client, 'untrack_file', { path: 'd.json' });
      expect(removed.status).toBe('ok');
      expect((removed.data as { removed: boolean }).removed).toBe(true);
    });
  });
});
