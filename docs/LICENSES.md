# License research and choice

This repository is licensed under the **GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later)**. Full text: [`LICENSE`](../LICENSE).

## Why AGPLv3

- Copyleft license designed for software that can interact with users over a network (AGPL section 13). Modified versions running on a server must offer their Corresponding Source to users.
- Ensures improvements made to network-deployed versions remain available to the community, which matters for this POS product where hosted/cloud features are planned.
- Compatible with GPL-3.0 code (AGPLv3 section 13 permits linking with GPL-3.0 works).
- Requires preserving copyright notices and license text, and providing source on conveyance.

## Comparison: AGPLv3 vs Apache 2.0 vs MIT

| Aspect | AGPLv3 | Apache 2.0 | MIT |
|---|---|---|---|
| Type | Strong copyleft (network copyleft) | Permissive | Permissive |
| Commercial use | Yes, but derivatives must be open under same license | Yes | Yes |
| Modification/distribution | Allowed; modified source must be shared, including network use | Allowed with attribution and NOTICE preservation | Allowed with copyright notice |
| Patent grant | Implicit via copyright; patent retaliation? AGPL has patent provisions (section 11) | Explicit patent grant with termination on patent litigation | None |
| Trademark | Not granted | Explicitly not granted (section 6) | Not granted |
| Sublicensing | Not allowed | Allowed | Allowed |
| Warranty/liability | Disclaimer (sections 15-16) | Disclaimer (sections 7-8) | Disclaimer |
| Source offer for network use | Required (section 13) | No | No |
| Best fit | Hosted SaaS where you want derivatives to stay open | Libraries/platforms wanting broad adoption + patent protection | Simple, minimal restrictions |

## When to prefer each

- **AGPLv3** (chosen here): you want to prevent proprietary forks of a network-facing application while still allowing use, study, modification, and redistribution.
- **Apache 2.0**: you want maximum adoption and an explicit patent grant; common for infrastructure/libraries. Allows proprietary derivatives.
- **MIT**: simplest permissive license; minimal friction; no patent grant; common for small libraries and tooling.

## Implementation notes

- Root `LICENSE` contains the AGPLv3 text.
- Workspace metadata updated: `Cargo.toml` (`license = "AGPL-3.0-or-later"`), `apps/desktop/package.json`, and `apps/desktop/src-tauri/Cargo.toml`.
- README documents the license.
- Per GNU guidance, each source file should carry a copyright line and license notice; SPDX identifiers (`SPDX-License-Identifier: AGPL-3.0-or-later`) may be added incrementally.
- Network-facing builds must provide a way for users to obtain Corresponding Source (AGPLv3 section 13), e.g. a "Source" link or bundled source archive.

## References

- GNU: How to Use GNU Licenses — https://www.gnu.org/licenses/gpl-howto.html
- GNU: License translations — https://www.gnu.org/licenses/translations.html
- GNU: License logos — https://www.gnu.org/graphics/license-logos.html
- GNU: GPL violations — http://gnu.org/licenses/gpl-violation.html
- AGPLv3 text — https://www.gnu.org/licenses/agpl-3.0.txt
- Apache 2.0 — https://www.apache.org/licenses/LICENSE-2.0
- MIT — https://opensource.org/licenses/MIT
