# Mobile site audit — 2026-10-16

Scope: the APITOKEN2.0 frontend at `apitoken-2-0-admin.vercel.app`, after the navy/red/white palette update.
Delivery remains GitHub `master` → Vercel. No upstream host deployment is involved.

## Coverage

Browser checks used CSS viewports of 320×740, 390×844 and 768×1024, plus chart breakpoint checks at 820/821px and a 1366px desktop regression check.
Checks were interactive and targeted; this is not a claim that every URL was visually reviewed.

- Russian and English landing headers, language/theme controls, mobile menu and Escape dismissal.
- Russian B2B comparison dialog at 320px, and English standalone B2B fields.
- React documentation at 320/768px: header actions, code captions/copy buttons and API language tabs.
- Local fixture dashboard: keys/create dialog, usage chart/day selection, balance, referrals, profile and support.
  The usage chart was measured at 320/390px; backdrop dismissal and focus restoration were reproduced and rechecked.
- Production-build auth forms at 320px: login, registration, recovery/reset, and verification with an artificial long email.
  Forms were inspected without submitting credentials or triggering messages.
- Direct production-build navigation to about, status, changelog, contacts, Russian plans, learning hub, terms and support.
  These pages had no page-level horizontal overflow at 320px. Intended scroll regions remain scrollable.

## Fixed findings

1. A late mobile wordmark rule pushed the landing menu button outside a 320px viewport. The final narrow breakpoint now fits all header controls.
2. B2B comparison columns expanded to their longest words and clipped the right column. Equal shrinkable tracks and word wrapping keep both columns readable.
3. Documentation code captions collapsed to 2px beside the copy button. Mobile code bars now reserve space for captions and hide decorative dots.
4. Documentation API language tabs extended beyond the content panel. Tabs now wrap inside the panel with usable touch height.
5. The documentation header overflowed at tablet widths. Secondary labels/back navigation now collapse while language, theme and dashboard actions remain visible.
6. Public React menu links had approximately 18px touch height. Mobile links now have a 44px minimum height.
7. Dashboard chart date labels were positioned against the app shell, far below the chart. Axis labels and hints now occupy local grid rows; Y-axis ticks no longer stretch to the full plot height. Phones show the two endpoint dates so full Russian referral dates do not overlap.
8. Chart day details clipped provider rows and squeezed mobile metrics. Cards now grow with content and metrics rearrange on narrow screens. Plot heights remain 260px on desktop and 220px on phones.
9. Closing the dashboard menu through its backdrop left focus on a hidden button. Cleanup returns focus to the menu trigger while preserving deliberate focus on a navigation destination.
10. Long auth messages/email addresses and dynamic key-dialog names lacked safe wrapping. They now stay inside their cards.
11. About, status and changelog used shared card classes without loading their stylesheet. Direct visits now load the existing responsive documentation styles.
12. Reduced-motion CSS disabled tutorial text animations while leaving their initial opacity at zero. Text is now visible; static docs/B2B anchor scrolling also respects reduced motion. These branches were source-reviewed, not browser-emulated.

Static documentation copy controls also received a 40px touch target. The existing mobile palette checker was updated to expect the current navy/white/charcoal colors.

## Validation

- Next.js production build passed.
- TypeScript check and scoped ESLint passed.
- Vitest: 211/212 passed, including three new mounted React/JSDOM focus regression tests.
  The remaining failure predates this change: `migration.test.ts` expects the removed `overview-pricing-facts` class.
- `git diff --check` passed. Static JavaScript syntax checks passed.
- Measured after correction: B2B rows/cells have matching client/scroll widths at 320px; documentation captions have real width and tabs fit; chart axes have local width and mobile detail client/scroll heights match.

## Limits

Testing used a Chromium browser with resized viewports, not physical iOS/Android devices. Software keyboard, safe-area behavior and screen-reader announcements still require device testing.
Dashboard data/actions used local fixtures; live authentication, payments, real key changes, mail delivery and external support services were not exercised.
Reduced-motion behavior was checked in source because the available browser controls did not expose that emulation setting.
