# Smart POS interface system

## Intent

- **Human:** A cashier on a busy independent retail counter, with a manager reviewing products and cash closeout.
- **Main job:** Find/scan an item, confirm quantity and price, take payment, and hand over a clear receipt without losing the current sale.
- **Feel:** Steady, tactile, and legible under shop lighting. The register should feel like a well-organized counter, not a generic analytics dashboard.

## Visual choices

- **Palette:** Ink `#202A27` for working text and navigation; warm paper `#F5F1E8` for the room; chalk `#FFFEFA` for cards; pine `#24634D` for checkout and positive status; clay `#C65C3A` for primary attention and stock issues; brass `#C69236` for low-stock/warning; slate `#64716B` for supporting data.
- **Why:** Paper and ink keep dense product rows readable; pine carries a dependable completion signal; clay reserves urgency for an action that needs attention; brass signals “watch” without looking like an error.
- **Depth:** Borders-only, with surface shifts. The register must stay readable in bright shop light and avoid decorative elevation.
- **Surfaces:** Main workspace warm paper; catalog and modal sheets chalk; navigation ink; cart ticket a lightly tinted paper with a perforation rule.
- **Typography:** System UI (`Segoe UI`, `Inter`, sans-serif) for reliable local rendering; medium-weight labels; bold totals; tabular numerals for prices and quantities.
- **Spacing:** 4 px base, 8 px control gaps, 16 px group padding, 24 px section breaks.
- **Radius:** 6 px controls, 10 px cards, 14 px dialogs. Avoid pill-shaped buttons except compact status badges.
- **Signature:** The right-side sale ticket, with aligned quantity/price columns and a perforated subtotal edge, keeps the active transaction visible while staff browse the catalog.

## Interaction rules

- Barcode and SKU search remain available without leaving the register.
- The active cart survives navigation to product lookup and settings.
- Show loading, error, empty, hover, focus, disabled, and success states for every data action.
- Confirm only actions that void/refund a sale, remove inventory records, or close a cash day.
- Never seed a real-looking business, user, or transaction. First run starts empty and guides store setup.
- At narrow window sizes, keep the cart reachable with a persistent order button and an explicit total.
