# Mobile site audit — 2026-09-08

Scope: the APITOKEN2.0 frontend fork deployed at `apitoken-2-0-admin.vercel.app`.
Delivery for this fork is GitHub `master` → Vercel; no standalone server or host deployment was used.

## Coverage

- 41 route/template groups × four viewport/language/theme combinations: 164 browser cases.
  Widths: 320, 390 and 768 CSS px; English/Russian; light/dark; reduced motion.
- Landing, static documentation, B2B, all seven dashboard sections, Next documentation,
  plans/models, all eight integration guides, legal/support/about/contacts/changelog/blog/status,
  calculator, learning articles, model detail, error directory/tool/article, Korean/Chinese
  learning hubs and the 404 template.
- 33 additional auth-page cases: login, registration, password recovery/reset, email verification
  in EN/RU, and the shared OAuth callback, at 320/390/768 px. A separate local non-fixture dev
  server exposed these pages; backend requests were intercepted with 401 responses.
- 30 navigation scenarios: five menu implementations × EN/RU × 320×568, 390×844, 844×390.
  Escape, link selection, backdrop, keyboard cycling, focus return, background scroll restoration,
  hidden-panel inertness and desktop/mobile breakpoint transitions were checked.
- Key creation dialog layout/close at 320/390 px, including a 320 px viewport height to simulate
  reduced space beside a keyboard; fixture-only profile save and invalid credit amounts.
- 503 distinct internal addresses from rendered links and the sitemap returned HTTP 200 locally.
  This is a reachability check, not a visual inspection of every article.

## Fixed findings

1. Landing menus stayed open after link selection or Escape and left scrolling locked.
2. Documentation menu state disagreed with `aria-expanded`; docs/B2B allowed background scrolling.
3. Dashboard lacked Escape handling, expanded/control attributes and an internal close button.
   Off-screen links remained keyboard-accessible. Public React menus lacked background locking.
4. All menu systems now restore scroll state, close at desktop breakpoints, contain keyboard
   navigation and exclude hidden content from interaction. Small-height panels scroll independently.
5. Small input fonts caused an iOS focus-zoom risk in B2B, dashboard search/sort/profile/referral,
   learning search and auth forms. Editable mobile fields now meet the 16 px floor.
6. Profile copy, support CTA and calculator CTA labels were clipped at 320 px.
7. Contacts reused documentation cards without loading their stylesheet, producing unstyled cards
   and long-email overflow. The page now loads the existing shared styles.
8. A later desktop CSS rule overrode the mobile model-rate grid. The final mobile rule now stacks it.
9. Referral chart edge dates extended beyond the chart; edge alignment and a two-label narrow
   axis now keep dates readable. Key sorting can shrink without overflowing its flex row.
10. Empty landing login/start/guides/status/error/legal links now use existing localized routes.
    English B2B quote buttons open the offer form; public navigation uses the actual `#start` anchor.
11. B2B falsely reported “Sent” without a request. It now prepares an encoded email draft with
    explicit manual-send instructions and a fallback email address. No endpoint was invented.
12. GSAP continued animations with reduced motion enabled. That mode now retains original,
    readable markup without animated line masks or timelines.

## Verification and reproduction

`apps/web/scripts/audit-mobile.mjs` saves JSON findings/screenshots and fails on measured overflow,
clipping, undersized form text, missing same-page anchors, unexpected status or runtime errors.
Intentional scroll containers and accessibility-only text are excluded.
`apps/web/scripts/test-mobile-interactions.mjs` asserts the menu and form interactions above.
See `apps/web/VISUAL_AUDIT.md` for setup. Browser artifacts remain ignored in `.artifacts/`.

Final local layout scan: 164/164 passed. Auth scan: 33/33 without measured issues or page errors.
Navigation/forms passed. Next production build, TypeScript, scoped ESLint and all 204 web unit tests passed.
The B2B handoff also has EN/RU unit tests that verify encoding and the absence of network delivery.

## Limits

Chrome mobile emulation was used, not physical iOS Safari or Android devices. A real software keyboard,
notch/browser-toolbar behavior and assistive-technology announcements still need device testing.
Dynamic article families were sampled visually, not individually reviewed in every locale.
Live authentication, payments, key mutations, email delivery, Telegram and the external OpenKeys
backend were not exercised. Dashboard actions used local fixtures. External availability and backend
business logic are outside this frontend-only audit; a Vercel deployment does not create those services.
