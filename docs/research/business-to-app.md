# Business to app: specification and challenge

Status: 2026-11-28. Proposal only, no code. Reuben's idea, 2026-11-28 (project thread "Plan the business-to-app platform"): turn a local business's public presence into a structured business profile and API, use it to make a branded native iPhone app on ixOS, and sell by showing a working preview on his phone before the business has agreed to anything. Working name in his message: BusinessForge.

This page checks the idea, and Reuben's follow-up v1.0 specification, against what ixOS already has and against Apple's rules, keeps what is strong, changes what isn't, and ends with a tiny first milestone and the decisions that are his.

## Summary

1. **The sales tactic is the best part, and it costs almost nothing to test.** A preview on the phone with the business's own name, colours, services and prices is a far stronger opener than "would you like an app?". The first milestones should prove that tactic works, not build a platform.
2. **Don't build a generator; build one native template fed by a verified profile.** A "services business" template (about, services and prices, opening hours, contact, news, a book button) covers riding schools, hairdressers, trades and most professional services. The profile is data, so a new prospect needs no new code, no build and no App ID. It runs inside Device Lab, so it needs no phone slot.
3. **Most of the proposed stack isn't needed until there are paying customers.** No FastAPI, Postgres, Redis, S3, Crawlee, Firecrawl, n8n or six services for the first milestones: one extraction command that fetches a handful of pages, reads the structured data they already carry, fills a typed profile with an LLM, records where every field came from, and waits for a person to approve it.
4. **The production app is where the plan is weakest, and it isn't blocked by code.** Apple rejects apps that are mostly brochures (guideline 4.2.2) and template apps submitted on a client's behalf (4.2.6). So a client app needs a real function (bookings, loyalty, an account) and the client's own paid Apple developer account. On top of that, ixOS has never shipped to the App Store and Reuben's own team is free. The first paid offer may have to be something other than an App Store app (decision D4).
5. **Many target businesses already have a branded app offer from their booking provider** (Mindbody, Phorest and others for gyms and salons; riding schools have Equestrian Systems, EC Pro and a new ABRS system). The gap to look for is businesses with no platform app, and what they would actually pay for is a question for the sales test, not an assumption.
6. **Time-box it against eSIM, VPN and Travel Hub.** Engineering for M0 and M1 is small; the expensive part is Reuben's evenings visiting businesses. The BusinessForge repository holds only the schema and the extraction tool until then; the server, dashboard and integrations wait until five real conversations say there is something to sell (gate at M2).

## Reuben's v1.0 specification, reviewed

Reuben followed up the same day with "BusinessForge: technical specification and implementation roadmap, version 1.0" (project thread, 2026-11-28 12:19). It is careful and mostly right; this section says what this page takes from it, and where it changes it. The rest of the page is the refined version (call it v1.1).

### Adopted from v1.0

- **Field-level facts with provenance**, and verification separate from confidence ("even a highly confident AI extraction is not the same as business-owner approval"). The confidence bands (0.95, 0.80, 0.50) are adopted as starting thresholds, to be calibrated in M0.
- **Conflicts are never resolved silently** (£35 on the home page, £40 on the pricing page: needs review), and stale content (old news, expired offers) is flagged.
- **Extraction safety:** page content is data, never instructions; SSRF protection, no private or metadata addresses, redirect and size limits, crawl budget, isolated browser, no credentials near the model. Plus the test with a page that carries malicious instructions.
- **The LLM never writes Swift.** It fills a validated document that selects known components.
- **Approved snapshots for meetings**: nothing changes a prospect's preview after approval, and a permanent preview indicator.
- **No leakage between prospects**: a test that switching never shows one business's logo or content in another's preview.
- **A design-quality check** before a preview is ready (missing branding, poor images, empty screens, missing accessibility labels): a preview that fails is `needs_review`.
- **A booking state machine** that never reports `confirmed` because our own server accepted a request, and the business stays merchant of record.
- **Prospect states with entry conditions** (discovered to production), and the main commercial metric: **minutes from URL to presentation-ready preview**.
- **Python for crawling and extraction, Rust for contracts and the app.** The schema is defined once in Rust and exported as JSON Schema.
- **Guideline 4.3 (spam)** alongside 4.2.6: many near-identical client apps are a rejection risk even under each client's account.

### Changed from v1.0

| v1.0 | Change | Why |
|---|---|---|
| A dedicated "BusinessForge Showcase" app installed permanently on the phone | **A Business Preview module inside Device Lab** until there is a paid Apple team | All three free-team slots are full (Device Lab, WebDriverAgent, VPN). A standalone app means removing one of them, and a new App ID. Device Lab already installs every app at once by cable |
| Showcase lives in the BusinessForge repository | **The preview's Rust crate and SwiftUI screens live in ixOS `examples/business-preview`**, hosted by Device Lab; it reads the profile schema crate from BusinessForge by git (D2, decided: the schema crate lives there from the start) | Device Lab can only host apps in ixOS: ixOS depending on a BusinessForge Swift package would run the dependency the wrong way. It moves to the BusinessForge repo as its own app when a paid team gives it a slot |
| M0 to M3 (PostgreSQL, multi-tenant API with row-level security, Redis workers, dashboard) before the phone shows anything; 16 to 25 days of work before a sales meeting | **A sales gate after the first preview** (M2 below); the server, database, tenants and dashboard come after it | None of that is needed to show five owners their app, and all of it is wasted if none of them want to pay. Packs as files, served from the existing Hetzner box or sent with Relay, are enough |
| Two templates (Universal and Equestrian) in the MVP | **One template ("services business") with category vocabulary**; equestrian is a category of it (Horses as its Team screen, Shows as its News) | Riding schools, salons and trades share the same five screens. A second template is earned by a real prospect that doesn't fit (a restaurant menu, a shop catalogue) |
| Per-client production apps made with `forge init`, each with its own Rust core | **One client app project, built per client with a different manifest, bundle id, name and icon** | `forge init` per client is a codebase per client, the thing v1.0's own principles avoid. One project keeps every client on the same fixes |
| The MVP includes an "initial build workflow" for production apps | **Not before D4** | It needs a paid team, the App Store release route ixOS hasn't built yet, and a real function per 4.2.2. Until a business asks for it, it is speculative |
| A public multi-tenant Business API from M2 | **The schema is the contract; an API when a second consumer exists** (a client app, or the profile service publishing to Google and the website) | Tenancy, authentication and row-level security for zero tenants is cost without evidence |
| Estimates of 2 to 8 days per milestone | **No dates; measure M0** | ixOS work hasn't matched estimates, and Reuben's time for meetings is the real limit |
| The profile as the backend for the app | **Also test the profile as a product on its own** (finding 3 below) | It needs no App Store, no Apple account and no per-client binary |

## What to keep from the first plan

| Reuben's point | Verdict |
|---|---|
| Show a working app before asking for anything | **Keep, make it the product's first milestone** |
| Distinguish extracted facts from actions that can be performed; never present scraped data as live | **Keep, and enforce it in the schema** (every field has a kind: static, link, live) |
| Provenance on every field (source, date, confidence, verified, public or authorised, static or live) | **Keep, from day one.** It is cheap now and impossible to add later |
| Official APIs first, scraping only for public facts, browser automation as fallback, manual approval when unsure | **Keep** |
| One configuration, many businesses; not hundreds of codebases | **Keep**, but as templates plus data, not a generated app (below) |
| A demo app holding several prospects, client-owned apps for production | **Keep** for the demo. Production needs more thought (D4) |
| Modular monolith, not microservices | **Keep**, and smaller than proposed |
| Start with one industry | **Keep**, but pick by template, not by industry (below) |
| Recurring revenue, not one-off builds | **Keep**; the price list is a hypothesis for M2 |

## Where I'd change it

### 1. Templates plus data, not a generator

The plan calls for a "configurable app renderer" that assembles a native experience from a configuration. ixOS already has two nearby things, and neither is the right base:

- **A2UI** (Device Lab's A2UI Lab, [assessment](external-projects/assessments/ai/a2ui.md)) lets an agent compose a surface from eight components, which the person confirms. It exists for agent proposals with a confirm step. A business app has a fixed layout and changing data, so A2UI would add a protocol, a gate and a confirm step to something that needs none of them, and its eight components can't draw a branded home screen.
- **App Studio** ([spec](app-studio.md)) has an agent write a new Forge project. That is per-client code, exactly what this plan wants to avoid.

The right base is the ordinary ixOS pattern: **Rust owns a typed `BusinessProfile`, validates it and serves it; SwiftUI templates built on ForgeUI present it.** A new business is a new profile, not new code. A new kind of business is a new template, added by a person when a real prospect needs it. This is also what App Review is comfortable with: the app downloads data, not code or layouts (guideline 2.5.2).

Start with **one template, "services business"**, rather than one industry:

| Screen | From the profile | Riding school | Hairdresser | Tradesperson |
|---|---|---|---|---|
| Home | name, tagline, hero image, colours, logo, primary action | Book a lesson | Book an appointment | Ask for a quote |
| Services | services with prices, durations, notes | Lessons, livery, pony days | Cuts, colour | Jobs, call-out fee |
| Team (optional) | people or animals with photos | Horses and instructors | Stylists | none |
| News (optional) | posts with dates | Shows, closures | Offers | none |
| Visit | address, map, opening hours, phone, email, social links | ✓ | ✓ | ✓ |

The vocabulary (lesson, appointment, quote), icons and the optional screens come from the profile's category. Equestrian is a category of this template (Horses as the Team screen, Shows as News). Restaurants (menus, allergens) and shops (catalogue, stock) are different templates and wait until a prospect needs one.

### 2. The book button opens what they already have

In the preview, and in any first production app, "Book" opens the business's existing booking page in an in-app Safari view. That is honest (nothing pretends to be connected) and is also what a business with an existing booking system will want. Live availability and in-app booking are integrations, and each one exists only where a provider has an API and the business authorises it (stage 5 in the plan).

### 3. The profile may be the better product

The plan treats the profile as the backend for the app. It may be worth more on its own: one verified, maintained record of a business that can also publish to its Google Business Profile, to schema.org data on its website, and to the AI assistants that increasingly answer "where can I book a riding lesson near Livingston". That is a recurring service a business understands, needs no App Store and no Apple account, and the app becomes the most visible thing the profile powers. M2 should ask about both (D1).

### 4. Much less infrastructure first

| Proposed | Needed for M0 to M2? | Instead |
|---|---|---|
| Python + FastAPI, PostgreSQL, Redis queue, S3 | No | Profile packs as files; a database when there are customers |
| Crawlee, Scrapy, Firecrawl | No | Fetch the home page, sitemap and up to 20 linked pages; Playwright (already on this machine and the laptop) only for pages that need JavaScript |
| LLM structured extraction | Yes | Run inside a Claude Code session for M0 (no API key); an API key only when it is automated |
| n8n | No | Nothing to connect yet |
| Six services, prospect dashboard, customer portal | No | A prospects list in the preview app and one file per prospect |
| OpenAPI Business API | No | The JSON Schema of the profile is the contract; an API when a second consumer exists |

### 5. Distribution is the real blocker, not the extraction

| Fact | Consequence |
|---|---|
| 4.2.6: apps from a commercial template or app-generation service must be submitted by the content provider, or be one aggregated app | Each client app goes under the client's own Apple developer account: US$99 (about £79) a year, and an organisation enrolment needs a D-U-N-S number. Small businesses may balk at both |
| 4.2.2: apps shouldn't be primarily marketing material, web clippings or a collection of links | A brochure app (about, services, contact, a booking link) risks rejection. A production app needs a real function: booking, loyalty, membership, notifications |
| Reuben's team is free; ixOS has never shipped to the App Store ([first release plan](../first-app-store-release.md), stages R0 to R5 not started) | Before the first client app, ixOS needs a working release route from Linux, and Reuben needs a paid account to build it with |
| One aggregated app holding many businesses is allowed | It is a consumer marketplace that needs an audience; not a first product |

So the order is: preview (needs nothing new), sales test, then decide what the first paid offer is (D4), and only build the release route if it is a client App Store app.

## The first milestone in detail

**Goal:** from a website address to a verified profile to a preview in Device Lab on Reuben's iPhone, with minimal human time.

### Profile v0

One JSON document per business, defined once in Rust (serde types, JSON Schema exported for the extraction tool), validated by Rust on the phone.

```json
{
  "schema": "business-profile/0",
  "id": "riding-school-example",
  "category": "riding_school",
  "name": { "value": "The Riding School", "source": "s1", "method": "jsonld", "verified": true },
  "branding": {
    "primary": { "value": "#171717", "source": "s2", "method": "css", "verified": true },
    "logo": { "value": "images/logo.png", "source": "s1", "method": "html", "verified": true }
  },
  "services": [
    {
      "name": { "value": "Private lesson (30 min)", "source": "s3", "method": "html", "verified": true },
      "price": { "value": { "amount": "45.00", "currency": "GBP" }, "source": "s3", "method": "llm", "confidence": 0.8, "verified": false }
    }
  ],
  "actions": {
    "book": { "kind": "link", "value": "https://booking.example/riding-school", "source": "s4", "method": "html", "verified": true }
  },
  "sources": {
    "s1": { "url": "https://example.com/", "fetched": "2026-11-28T10:00:00Z" },
    "s2": { "url": "https://example.com/assets/site.css", "fetched": "2026-11-28T10:00:02Z" },
    "s3": { "url": "https://example.com/lessons", "fetched": "2026-11-28T10:00:05Z" },
    "s4": { "url": "https://example.com/book", "fetched": "2026-11-28T10:00:08Z" }
  },
  "preview": { "status": "concept", "consent": "none" }
}
```

Rules the schema enforces:

- Every value carries a source and a method (`jsonld`, `html`, `css`, `llm`, `manual`). An `llm` value is unverified until a person approves it, and the preview hides unverified prices.
- Every action has a kind: `link` (opens their page), `contact` (call, email, map) or `live` (an authorised integration; none in v0). There is no way to express a booking the app performs itself.
- `preview.status` is `concept` until the business agrees; the app shows a permanent "Concept preview, not an official app" label while it is.

### Extraction (M0)

One command in the BusinessForge repository, run by a person (`extract <url>`), in Python, validating against the JSON Schema the Rust crate exports (D2):

1. Fetch the home page, robots.txt, sitemap and up to 20 same-site pages, politely (identify the client, obey robots.txt, a few seconds between requests).
2. Read what is already structured: schema.org JSON-LD (LocalBusiness, Service, OpeningHoursSpecification, Offer), Open Graph, the web manifest, favicon and logo, theme colours.
3. Ask an LLM to fill the rest of the profile from the page text, citing the source page for every field, and to leave a field empty rather than guess.
4. Write the profile and its images as a **profile pack** (a folder or zip with `profile.json`, images and hashes) plus a review list of unverified fields.
5. A person approves or corrects fields in the review list. **Measure the minutes.**

M0 exit: 5 real sites of local businesses (at least 2 riding schools), each reviewed in under 15 minutes, with no wrong price or opening time left after review.

### Business Preview in Device Lab (M1)

A new app module in Device Lab (Apps screen), like every other ixOS app, so it needs no phone slot and no new App ID.

- **Prospects list:** each imported pack, with its review state. Swipe to delete.
- **Import:** from Files or the share sheet, or by URL. Reuben can send a pack from the laptop with Relay. Packs are cached on the phone, so the preview works with no signal in a stable yard.
- **The template:** the five screens above in the business's colours, ForgeUI components, Dark Mode and Dynamic Type, accessibility identifiers as every app has them.
- **Actions:** call, email, map, and Book opening their own page. Nothing else.
- **Label:** "Concept preview, not an official app" on every screen while `preview.status` is `concept`.
- **Presenter mode:** hides the prospects list and Device Lab chrome, so the owner sees only their app. One long press to leave.

Rust owns pack validation (hashes, schema version, image sizes, the label rule) and the store; Swift presents it. Phone check, about 10 minutes: import a pack, every screen in light and dark at the largest text size, Book opens the right page, presenter mode, delete.

### Sales test (M2): the gate

Reuben shows the preview to 5 businesses, at least 3 of the same kind. For each: did the preview get attention, what do they use for booking now, would they pay, and for what (an app, the maintained profile, bookings, loyalty), and how much. Record it in one table.

**Gate:** at least 2 businesses ask for a paid next step. If none do, stop or change the offer before writing any more code. If they do, decide D4 and add the server pieces to BusinessForge.

## Later stages (only after the gate)

| Stage | What | Depends on |
|---|---|---|
| M3 | First paid offer, whichever D4 picks | Gate passed |
| M4 | Extraction as a service and a prospects dashboard, in BusinessForge | More than a handful of prospects |
| M5 | One integration, chosen by the first customer's booking provider and only with a provider API and their authorisation | A customer who needs it |
| M6 | Client App Store app under the client's account | ixOS release route (first-app-store-release R0 to R5), Reuben's paid account, a real function per 4.2.2 |
| M7 | Customer portal to edit their profile | Several customers |

## What it reuses

| Need | Already in ixOS |
|---|---|
| Hosting the preview without a slot | Device Lab (all apps in one install, by cable) |
| Look and behaviour | ForgeUI, layered navigation, appearance, haptics |
| Remote images, caching | ForgeImages |
| Getting packs onto the phone | Relay file transfer from the laptop, or a URL |
| Open a booking page | in-app Safari view (as other apps already do for links) |
| Rust owns state, Swift presents | the app crate pattern ([app-crate.md](../app-crate.md)) |
| Extraction with an LLM, no API key at first | a Claude Code session |

What it does **not** reuse: A2UI and App Studio, for the reasons in section 1. If a later stage wants an agent to propose profile changes for the owner to confirm, that is exactly A2UI's pattern and can be added then.

## Legal and compliance

Not legal advice; these need checking before any business is approached, and again before the first paid customer.

- **Not an official app.** The preview never leaves Reuben's phone, carries the concept label, and is never published, shared as a build or shown publicly. Packs for businesses that decline are deleted.
- **Content and trademarks.** Their logo, photos and text are reused only for a private pitch to the owner. A production app needs their written licence for all content, and any photos they don't own (stock, a photographer) need checking.
- **Crawling.** Obey robots.txt and site terms, rate-limit, identify the client, public pages only, no logins, no bypassing anti-bot measures.
- **UK GDPR.** A sole trader's name, phone and email are personal data. A prospects list is personal data held for direct marketing: write a legitimate interests assessment, keep only what the pitch needs, delete on request, and register with the ICO (data protection fee) once the company exists. Staff photos and names on Team screens need the business's consent in production.
- **PECR.** Cold email or texts to sole traders and partnerships need consent; walking in or phoning a business number is the safer opener.
- **Production.** A contract (who owns the app, the Apple account, the content, the data), a privacy policy per app, App Store privacy labels, and processor terms if the app ever holds their customers' data.
- **Apple.** 4.2.6 and 4.2.2 as above; the free team's 7-day signing is fine for a preview on Reuben's own phone and nothing else.

## Risks

| Risk | Mitigation |
|---|---|
| Businesses like the demo but won't pay for an app | M2 asks about the profile offer too; the gate stops the build |
| Booking providers already sell branded apps | Target businesses with no platform app; integrate with their provider rather than compete |
| Wrong price or hours shown to an owner | Unverified LLM values hidden; human review is part of M0's measure |
| App Review rejects a client app as a brochure | No client app without a real function; decide in D4 |
| Production blocked by Apple account and release route | Known before M3; offer something that doesn't need it first if necessary |
| It pulls time from eSIM, VPN and Travel Hub | M0 and M1 are small; the gate is the time box |
| Each prospect takes hours | M0 measures minutes per profile; a template that needs per-prospect work has failed |

## Decisions for Reuben

| # | Decision | Recommendation |
|---|---|---|
| D1 | What the first paid offer is tested as at M2 | Test both: the app and the maintained profile. Don't decide before talking to 5 businesses |
| D2 | The new GeneralKnowledge/BusinessForge repository (v1.0 asks for it now) | **Decided 2026-11-28 (Reuben): create it now, schema and extractor only.** It holds the profile schema crate (Rust, exports JSON Schema), the extraction tool (Python) and docs. No server, database or dashboard until the gate. Packs of real prospects stay in project files, never in the repository. The ixOS preview (M1) depends on the schema crate by git |
| D2b | Where the phone preview lives | A module in Device Lab (ixOS `examples/business-preview`), so it needs no slot and no App ID. It becomes its own "Showcase" app in BusinessForge only when a paid team frees slots |
| D3 | First kind of business | Riding schools, because you have access to them, using the services template so hairdressers and trades come free |
| D4 | First production route (after the gate) | Decide after M2. Options: a client App Store app with a real function (needs your paid account, the release route and their account), the profile service with a web page, or both |
| D5 | Time box | M0 and M1 in about a week of thread work; M2 when you have time to visit; nothing after that without the gate |

## Sources

- Reuben's message and his BusinessForge v1.0 specification, 2026-11-28, in the project thread.
- Apple, App Store Review Guidelines 4.2.2, 4.2.6, 2.5.2; [developer forum on 4.2.6 and per-studio branded builds](https://developer.apple.com/forums/thread/840982); [GoodBarber on 4.2.6 for app builders](https://www.goodbarber.com/blog/app-store-4-2-6-rule-and-app-builders-a885/).
- Existing branded app offers: [Mindbody branded apps](https://www.mindbodyonline.com/en-gb/business/branded-apps), [Phorest](https://www.phorest.com/?p=6456).
- Riding school software: [Equestrian Systems booking system](https://www.capterra.co.uk/software/215488/equestrian-systems-booking-system), [ABRS riding school management system](https://www.britishequestrian.org.uk/news/abrs-launches-riding-school-management-system-to-support-centres-through-rising-costs), [EC Pro](https://pelham.pcuk.org/ec-pro/).
- ixOS: [A2UI assessment](external-projects/assessments/ai/a2ui.md), [App Studio](app-studio.md), [app capability audit](app-capability-audit.md), [first App Store release plan](../first-app-store-release.md).
