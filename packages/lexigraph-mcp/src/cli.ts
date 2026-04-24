#!/usr/bin/env node
// Lexigraph MCP server CLI.
//
// Usage:
//   lexigraph-mcp [--root <dir>]
//
// Defaults to the current working directory. Speaks MCP over stdio. All diagnostics go
// to stderr — stdout carries the JSON-RPC framing and must not be polluted.

import { resolve } from 'node:path';

import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';

import { createServer } from './server.js';

interface CliArgs {
  root: string;
}

function parseArgs(argv: readonly string[]): CliArgs {
  let root = process.cwd();
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--root' || arg === '-r') {
      const next = argv[i + 1];
      if (!next) throw new Error(`${arg} requires a value`);
      root = resolve(next);
      i++;
    } else if (arg === '--help' || arg === '-h') {
      process.stderr.write('Usage: lexigraph-mcp [--root <dir>]\n');
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  return { root };
}

async function main(): Promise<void> {
  const args = parseArgs(process.argv.slice(2));
  const { server } = createServer({ root: args.root });
  const transport = new StdioServerTransport();
  await server.connect(transport);
  console.error(`lexigraph-mcp: ready (root=${args.root})`);
}

main().catch((err: unknown) => {
  const msg = err instanceof Error ? (err.stack ?? err.message) : String(err);
  console.error(`lexigraph-mcp: fatal: ${msg}`);
  process.exit(1);
});
