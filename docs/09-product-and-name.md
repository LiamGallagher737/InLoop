# 09 Product and name

## Name

**InLoop**, from "human in the loop" (D-KBG2). It fits the core rule that decisions come from the human, and the PM workflow's rule that work lands only when the human says so. The command is `inloop`.

Availability checked on 2026-11-14 (domains from a daily index, not a live registry check; trademarks not yet checked):

| Name | Status |
|---|---|
| `inloop` on crates.io | Free |
| `inloop.sh` | Not registered |
| `inloop.dev` | Registered, listed for sale (make an offer) |

Before registering a domain or publishing the crate: check trademarks, and register `inloop.sh` if it's still free.

Other names considered: `crewlog` (crate free, `crewlog.dev` and `.sh` free), `hitl` (crate free, `usehitl.com` and `hitl.so` free, but a generic industry term and hard to trademark or search for), `logwright`, `keelnote`, `mainstay`, `bosun`.

## Licence

Closed source for now (D-VMRP, proposed). Liam owns all the code outright, so any licence stays possible later. If parts are opened, the redaction engine is the best first candidate: useful to many people on its own, and safer with outside review.

## A possible business later

Session capture on its own is a crowded space. What InLoop combines is less common: decisions agents can't collide on, an optional PM workflow with computed conflict checks, and strong redaction with client-side encryption.

The likely shape (D-VMRP, proposed):

- **Free:** the local CLI and MCP server, with local or bring-your-own-bucket session storage.
- **Paid:** hosted team features: instant board claims across people, a combined team view, notifications, a web viewer for decisions and sessions, and team key management.

Rules for now:

- Don't build for the business yet. Dogfood on Adpatch and Cirkit with co-founders first.
- Keep InLoop fully separate from Adpatch and LJPG's other projects: its own accounts, its own infrastructure, choices made for what suits InLoop.
- Keep client-side encryption, so a hosted service would never see plaintext. That's a selling point and limits liability.
