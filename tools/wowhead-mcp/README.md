# wowhead-classic MCP server

A local [MCP](https://modelcontextprotocol.io) stdio server that looks up WoW
Classic spell and item data (cast times, mana, ranges, icons, item stats) from
Wowhead's Classic tooltip API. Used while implementing abilities and items.

Wired into the repo via `.mcp.json` at the project root:
`node tools/wowhead-mcp/dist/index.js`.

## Setup (required on a fresh checkout)

`dist/` and `node_modules/` are gitignored, so the server must be built before
first use:

```bash
npm install
npm run build      # tsc: src/index.ts -> dist/index.js
```

Then reconnect the MCP in your client (`/mcp` → reconnect `wowhead-classic`, or
restart). If you skip this, the client reports `-32000` on connect — that's just
Node failing to find `dist/index.js`.

## Verify

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}' | node dist/index.js
# -> "Wowhead Classic MCP server running" + a JSON initialize result
```

## Notes

- Fetches live from `https://nether.wowhead.com/classic/tooltip/{spell,item}`.
- Icon URLs are `https://wow.zamimg.com/images/wow/icons/large/<icon>.jpg`
  (the `<icon>` name comes from the tooltip response — e.g. Crippling Poison is
  `ability_poisonsting`, not `ability_poisons` which is Instant Poison).
