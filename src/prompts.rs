// Prompt content generators for the three MCP prompts.
//
// Each function returns a fully-formed prompt string that guides Claude
// through a specific phase of the App Store screenshot pipeline.
// The actual `#[prompt_router]` wiring lives in `server.rs`.

// ---------------------------------------------------------------------------
// 1. prepare-app
// ---------------------------------------------------------------------------

/// Generate the `prepare-app` prompt content.
///
/// Guides Claude to analyze the app, define a `ScreenshotMode` enum, and
/// create a `ScreenshotDataProvider` that populates demo data for each mode.
pub(crate) fn prepare_app_content(bundle_id: &str, screens_count: u8) -> String {
    format!(
        r#"You are preparing the iOS app `{bundle_id}` for automated App Store screenshot generation.

Your goal: define exactly {screens_count} screenshot screens that best showcase this app's value proposition, then produce the Swift code that drives them.

## Step 1 — Analyze the app

Examine the project source code to understand:
- What problem does the app solve?
- What are the key user-facing features?
- Which screens convey the most value at first glance?

Prioritize screens in this order:
1. **Hero screen** — the single most compelling view (dashboard, main feed, etc.)
2. **Feature screens** — screens that highlight differentiating capabilities
3. **Detail screens** — settings, customization, or secondary features

Aim for {screens_count} screens total. Each screen must show a DISTINCT value proposition — never repeat the same message with minor variations.

## Step 2 — Create the ScreenshotMode enum

Create a Swift enum in a new file (e.g. `ScreenshotMode.swift`):

```swift
#if DEBUG
import Foundation

/// Modes for App Store screenshot generation.
/// Each case corresponds to one screenshot in the App Store listing.
enum ScreenshotMode: Int, CaseIterable {{
    // Example — replace with actual modes:
    case dashboard = 1
    case tracking  = 2
    case insights  = 3
    case settings  = 4
    case widgets   = 5
}}
#endif
```

Rules:
- Wrap EVERYTHING in `#if DEBUG` — none of this ships to production.
- Cases must be numbered starting from 1 (`.rawValue` maps to the screenshot index).
- Use descriptive, camelCase names that match the screen's purpose.
- Add a brief doc comment for each case explaining what the screen shows.

## Step 3 — Create ScreenshotDataProvider

Create `ScreenshotDataProvider.swift`:

```swift
#if DEBUG
import Foundation

/// Populates the app with realistic demo data for each screenshot mode.
enum ScreenshotDataProvider {{
    static func configure(for mode: ScreenshotMode) {{
        switch mode {{
        case .dashboard:
            // Populate demo dashboard data
            break
        case .tracking:
            // Populate demo tracking entries
            break
        // ... all cases
        }}
    }}
}}
#endif
```

The data must look REALISTIC — use plausible names, dates, numbers. Screenshots with placeholder data ("Lorem ipsum", "Test User") look unprofessional and hurt conversion.

## Step 4 — Wire launch argument handling

In the app's entry point (AppDelegate or @main App struct), add:

```swift
#if DEBUG
private func handleScreenshotMode() {{
    let args = ProcessInfo.processInfo.arguments
    for mode in ScreenshotMode.allCases {{
        if args.contains("--screenshot-\(mode.rawValue)") {{
            ScreenshotDataProvider.configure(for: mode)
            // Navigate to the appropriate screen
            return
        }}
    }}
}}
#endif
```

## Step 5 — Update appshots.json

After creating the enum, update the project's `appshots.json` config so the `screens` array matches your enum:

```json
{{
  "bundleId": "{bundle_id}",
  "screens": [
    {{ "mode": 1, "name": "dashboard", "description": "Main dashboard overview" }},
    {{ "mode": 2, "name": "tracking",  "description": "Activity tracking view" }}
  ]
}}
```

## Output checklist

- [ ] `ScreenshotMode` enum with exactly {screens_count} cases
- [ ] `ScreenshotDataProvider` with realistic demo data for every mode
- [ ] Launch argument handling (`--screenshot-N`) wired into the app entry point
- [ ] All code wrapped in `#if DEBUG`
- [ ] `appshots.json` screens array updated to match the enum"#
    )
}

// ---------------------------------------------------------------------------
// 2. design-template
// ---------------------------------------------------------------------------

/// Generate the `design-template` prompt content.
///
/// Guides Claude to create a professional Typst template for App Store
/// screenshots, covering typography, color, layout, and RTL/CJK support.
pub(crate) fn design_template_content(bundle_id: &str, style: &str, per_screen: bool) -> String {
    let template_note = if per_screen {
        "You are creating PER-SCREEN templates. Each screen gets its own `.typ` file \
         at `appshots/templates/template-{{mode}}.typ`. Screens may have different \
         background colors or layouts, but must share a consistent visual language \
         (same fonts, same caption placement, same padding)."
    } else {
        "You are creating a SINGLE shared template at `appshots/template.typ`. \
         All screens use this one file. The template receives `sys.inputs` to \
         differentiate screens (background color, screenshot path, captions)."
    };

    format!(
        r#"You are designing a Typst template for App Store screenshots for `{bundle_id}`.
Desired style direction: **{style}**

{template_note}

After generating the template, call the `preview_design` tool to render a preview, review it, and iterate until the result is polished and professional.

---

# TEMPLATE STRUCTURE

Every template MUST follow this skeleton:

```typst
// === Page setup ===
#set page(width: {{page-width}}pt, height: {{page-height}}pt, margin: 0pt)
#set text(font: "SF Pro Display", lang: sys.inputs.at("lang", default: "en"))

// === Read inputs ===
#let caption-title = sys.inputs.at("caption_title")
#let caption-subtitle = sys.inputs.at("caption_subtitle", default: none)
#let screenshot-path = sys.inputs.at("screenshot_path", default: none)
#let bg-color-raw = sys.inputs.at("bg_color", default: "oklch(25%, 0.05, 270deg)")
#let text-dir = sys.inputs.at("text_direction", default: "ltr")

// === Auto-scale text helper ===
#let auto-text(body, max-size: 56pt, min-size: 20pt, target-width: 100%) = {{
  let current = max-size
  while current > min-size {{
    let m = measure(text(size: current, body))
    if m.width <= target-width {{ break }}
    current = current - 1pt
  }}
  text(size: current, body)
}}

// === Background ===
#rect(width: 100%, height: 100%, fill: oklch(25%, 0.05, 270deg))[
  // Caption area (top ~40%)
  #block(width: 100%, height: 40%, inset: (x: 40pt, top: 80pt))[
    #set text(dir: if text-dir == "rtl" {{ rtl }} else {{ ltr }})
    #set align(if text-dir == "rtl" {{ right }} else {{ left }})

    #auto-text(
      text(weight: 700, fill: oklch(98%, 0, 0deg), caption-title),
      max-size: 56pt, target-width: 100% - 80pt,
    )
    #v(12pt)
    #if caption-subtitle != none {{
      text(size: 24pt, weight: 400, fill: oklch(85%, 0, 0deg), caption-subtitle)
    }}
  ]

  // Screenshot area (bottom ~60%)
  #if screenshot-path != none {{
    place(
      bottom + center,
      dy: 30pt,
      image(screenshot-path, width: 85%),
    )
  }}
]
```

# PAGE SIZES

Set page dimensions based on the target device (all at 1/3 scale for Typst pt):

| Device | Pixels | Typst page size |
|--------|--------|-----------------|
| iPhone 6.9" | 1320 x 2868 | `width: 440pt, height: 956pt` |
| iPad 13" | 2064 x 2752 | `width: 688pt, height: 917.33pt` |

The template is rendered at 3x scale (`typst-render` pixel_per_pt = 3.0) to produce the final pixel-perfect output.

---

# TYPOGRAPHY RULES

## DO:
- Use the `auto-text` helper (with `measure()`) to auto-scale text so it fits the container width. This is MANDATORY — fixed font sizes WILL overflow on longer locales like German.
- Set max font size 56pt for titles, 24-28pt for subtitles, and let `auto-text` reduce until fit.
- Use font weight 700-800 for titles (bold, attention-grabbing).
- Use font weight 400 for subtitles (lighter, supporting contrast).
- Line height: 1.1-1.2 for titles, 1.3-1.4 for body/subtitles.
- Letter spacing: -0.02em to -0.01em for titles (tighter = more premium feel).
- Explicitly set the font family — never rely on Typst's default font.

## DON'T:
- DON'T use fixed font sizes without the `measure()` auto-scale pattern — text WILL overflow on German (30-40% longer than English) and other verbose locales.
- DON'T use more than 2 font weights in the entire template.
- DON'T center-align long text — use left-align for LTR, right-align for RTL.
- DON'T let text touch the edges — maintain minimum 40pt horizontal padding from screen edges.
- DON'T use Typst's default font — always specify the font family explicitly.

---

# COLOR RULES — OKLCH ONLY

ALL colors MUST use `oklch(L%, C, Hdeg)` notation. No hex. No RGB. No HSL. EVER.

## DO:
- Background: use 1-3 gradient stops with subtle chroma/lightness shifts for depth.
- Text on dark backgrounds: `oklch(98%, 0, 0deg)` (near-white).
- Text on light backgrounds: `oklch(15%, 0, 0deg)` (near-black).
- Ensure text/background contrast ratio >= 4.5:1 (WCAG AA).
- Derive accent colors from the app's brand hue.
- Keep total palette to 3 colors max: background gradient + text + optional accent.

## DON'T:
- DON'T use hex colors (`#fff`, `#000`, `#3b82f6`) — even though Typst supports them, this project mandates OKLCH exclusively.
- DON'T use RGB or HSL color spaces.
- DON'T use pure black `oklch(0%, 0, 0deg)` — use `oklch(10-15%, 0, 0deg)` for a softer look.
- DON'T use more than 3 distinct colors in the template.

---

# LAYOUT RULES

## DO:
- Reserve the top ~40% of the canvas for the caption area (title + subtitle).
- Reserve the bottom ~60% for the screenshot with device bezel.
- Let the screenshot overflow the bottom edge slightly (20-30pt `dy` offset) — this creates visual depth and is standard in professional App Store screenshots.
- Use `place()` for absolute positioning of the screenshot image.
- Set `margin: 0pt` on the page — manage all spacing manually for full control.
- Support RTL: read `sys.inputs.text_direction` and flip alignment accordingly.

## DON'T:
- DON'T use Typst's default page margins — always set `margin: 0pt`.
- DON'T hardcode pixel sizes — use pt units (1px = 1/3 pt at 3x retina).
- DON'T place text over the screenshot — keep caption area and screenshot area completely separate.
- DON'T use Typst's built-in page numbering or headers/footers.

---

# LOCALE & SCRIPT SUPPORT

The template must render correctly for all 39 App Store locales:
- **RTL locales** (ar-SA, he): text direction, alignment, and reading order must flip.
- **CJK locales** (ja, ko, zh-Hans, zh-Hant): may need different font family; text is typically shorter than English.
- **German (de-DE)**: text is 30-40% longer than English — the most demanding locale for overflow testing.
- **Thai (th)**: requires specific font support; line-breaking rules differ.

Always test with the longest expected caption text (simulate German-length strings).

---

# WHAT MAKES GREAT APP STORE SCREENSHOTS

1. **Value proposition, not feature name** — "Track your fitness goals" beats "Activity Tracker".
2. **Readable at thumbnail size** — if the title is unreadable at 200px width, the font is too small.
3. **Consistent visual language** — all screenshots should feel like they belong to the same family.
4. **Localized feel** — RTL alignment for Arabic/Hebrew, appropriate fonts for CJK.
5. **Device context** — bezel frames make screenshots look realistic and premium.
6. **Visual hierarchy** — title dominates, subtitle supports, screenshot proves.

---

# ITERATION WORKFLOW

1. Write the template `.typ` file.
2. Call `preview_design` to render a preview image.
3. Review the preview critically:
   - Is the title readable at thumbnail size?
   - Is there enough contrast between text and background?
   - Does the screenshot placement look balanced?
   - Is there sufficient padding on all sides?
4. Iterate: adjust sizes, colors, spacing, and re-preview.
5. Test with a long German caption to verify `auto-text` scaling works.
6. Test with an RTL locale (ar-SA) to verify alignment flips correctly.
7. Only finalize when the result looks like a top-tier App Store listing."#
    )
}

// ---------------------------------------------------------------------------
// 3. generate-screenshots
// ---------------------------------------------------------------------------

/// Generate the `generate-screenshots` prompt content.
///
/// Orchestrates the full 10-step pipeline: scan, analyze, plan, caption,
/// translate, validate, capture, compose, review, and deliver.
pub(crate) fn generate_screenshots_content(devices: &str, locales: &str, modes: &str) -> String {
    let device_filter = if devices.is_empty() {
        "all configured devices".to_owned()
    } else {
        format!("devices: {devices}")
    };
    let locale_filter = if locales.is_empty() {
        "all 39 App Store locales".to_owned()
    } else {
        format!("locales: {locales}")
    };
    let mode_filter = if modes.is_empty() {
        "all configured screens".to_owned()
    } else {
        format!("modes: {modes}")
    };

    format!(
        r#"You are generating App Store screenshots for the configured app.

Scope: {device_filter}, {locale_filter}, {mode_filter}.

Execute the following pipeline steps in order. Each step uses a specific MCP tool — call them sequentially, reviewing the output of each before proceeding.

---

## Step 1 — Scan project metadata

Call `scan_project` to discover existing fastlane metadata (keywords, name, subtitle) for all locales.

Review the output:
- Which locales have metadata?
- Which locales are missing keywords?
- What is the current keyword density?

---

## Step 2 — Analyze keyword gaps

Call `analyze_keywords` for en-US to identify:
- Keywords already used in title/subtitle
- Keywords in the keyword field not yet reflected in screenshot captions
- Competitor keywords that could be incorporated
- High-value keywords that are missing entirely

The goal: every important keyword should appear in at least one screenshot caption.

---

## Step 3 — Plan screenshot messaging

Call `plan_screens` to create the mode-to-keyword-to-messaging mapping:
- Each screen targets specific keywords
- No keyword should be orphaned (unused in any caption)
- Messaging angles should be diverse — don't repeat the same angle across screens
- Hero screen (mode 1) gets the highest-value keywords

---

## Step 4 — Generate English captions

Call `save_captions` with `locale: "en-US"` to write the English captions.

Caption writing rules:
- **Title**: 3-6 words. States a USER BENEFIT, not a feature name. Must be readable at thumbnail size.
- **Subtitle** (optional): 5-10 words. Expands on the title with a supporting detail.
- **Keyword** (optional): a target keyword to incorporate naturally into the title or subtitle.

Examples of GOOD vs BAD titles:
| BAD (feature name) | GOOD (user benefit) |
|---------------------|---------------------|
| "Activity Tracker" | "Reach Your Fitness Goals" |
| "Budget Manager" | "Save Money Effortlessly" |
| "Note Taking" | "Capture Ideas Instantly" |
| "Calendar View" | "Never Miss an Appointment" |

---

## Step 5 — Translate captions for all locales

For each non-English locale (in priority order — de-DE, ja, fr-FR, es-ES, zh-Hans, ko, then the rest):

1. Call `get_locale_keywords` to retrieve that locale's ASO keywords.
2. Translate the English captions, but DO NOT just translate word-for-word. Instead:
   - **Incorporate locale-specific keywords** naturally into the translated captions.
   - Adapt the messaging angle if a keyword fits better with a different phrasing.
   - Respect cultural norms (formal vs. informal address, etc.).
3. Call `save_captions` with the translated captions for that locale.

### Translation rules (CRITICAL for ASO):

- **German (de-DE)**: Text is 30-40% longer. Titles MUST be concise — prefer compound words. Use formal "Sie" unless the app targets a young audience.
- **Japanese (ja)**: Shorter than English. Can be more descriptive. Use appropriate politeness level.
- **Korean (ko)**: Similar length to Japanese. Honorifics matter.
- **Chinese Simplified (zh-Hans)**: Very concise. 4-character idioms can pack a lot of meaning.
- **Chinese Traditional (zh-Hant)**: Same as Simplified but different character set. Keywords may differ — always check locale keywords.
- **Arabic (ar-SA)**: RTL. Ensure text direction is correct. Translations tend to be similar length to English.
- **Hebrew (he)**: RTL. Often shorter than English.
- **French (fr-FR, fr-CA)**: ~15-20% longer than English. fr-CA uses different vocabulary from fr-FR.
- **Spanish (es-ES, es-MX)**: ~15% longer. es-MX uses "ustedes" where es-ES uses "vosotros".
- **Portuguese (pt-BR, pt-PT)**: pt-BR is more informal; pt-PT is more formal.
- **Thai (th)**: No spaces between words. Ensure line-breaking is correct.

---

## Step 6 — Validate layouts

Call `validate_layout` to check every template renders correctly with all locale captions.

Fix any issues:
- Text overflow → shorten the caption or adjust `auto-text` min-size
- Missing fonts → install or substitute
- Broken RTL → check `text_direction` input

Re-run validation after fixes until all checks pass.

---

## Step 7 — Capture screenshots from simulator

Call `capture_screenshots` to launch the app in the simulator for each mode and capture screenshots with device bezels.

The tool uses:
- `xcrun simctl boot` to start the simulator
- `xcrun simctl launch --screenshot-N` to trigger each mode
- `screencapture -o -l <window_id>` to capture with pixel-perfect Apple bezels

Verify each capture looks correct — the app should show the expected screen with demo data.

---

## Step 8 — Compose final screenshots

Call `compose_screenshots` to render all final images via the Typst template.

This combines:
- Background (from template)
- Caption text (from saved captions)
- Screenshot image (from captures, with device bezel)

Output goes to `fastlane/screenshots/{{locale}}/` in the format expected by `fastlane deliver`.

---

## Step 9 — Review output

Inspect the composed screenshots:
- Are captions readable at thumbnail size?
- Do all locales render correctly (especially RTL, CJK, Thai)?
- Are backgrounds consistent across screens?
- Do screenshots overflow the bottom edge as designed?

If any screenshot has issues, fix the root cause (template, caption, or capture) and re-run `compose_screenshots` for affected items only — use the `modes` and `locales` filters for granular regeneration.

---

## Step 10 — Deliver

When all screenshots are verified, call `run_deliver` to upload to App Store Connect.

---

## Summary of MCP tools used

| Step | Tool | Purpose |
|------|------|---------|
| 1 | `scan_project` | Discover fastlane metadata |
| 2 | `analyze_keywords` | Find keyword gaps |
| 3 | `plan_screens` | Map modes to keywords to messaging |
| 4 | `save_captions` | Write en-US captions |
| 5 | `get_locale_keywords` + `save_captions` | Translate + save per locale |
| 6 | `validate_layout` | Check all templates render |
| 7 | `capture_screenshots` | Capture from simulator |
| 8 | `compose_screenshots` | Render final PNGs via Typst |
| 9 | (manual review) | Inspect + fix + re-compose |
| 10 | `run_deliver` | Upload via fastlane |"#
    )
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- prepare-app -------------------------------------------------------

    #[test]
    fn prepare_app_contains_bundle_id() {
        let content = prepare_app_content("com.example.myapp", 5);
        assert!(content.contains("com.example.myapp"));
    }

    #[test]
    fn prepare_app_contains_screens_count() {
        let content = prepare_app_content("com.test", 7);
        assert!(content.contains("7"));
    }

    #[test]
    fn prepare_app_references_screenshot_mode_enum() {
        let content = prepare_app_content("com.test", 5);
        assert!(content.contains("ScreenshotMode"));
        assert!(content.contains("CaseIterable"));
    }

    #[test]
    fn prepare_app_references_data_provider() {
        let content = prepare_app_content("com.test", 5);
        assert!(content.contains("ScreenshotDataProvider"));
    }

    #[test]
    fn prepare_app_references_debug_guard() {
        let content = prepare_app_content("com.test", 5);
        assert!(content.contains("#if DEBUG"));
    }

    #[test]
    fn prepare_app_references_launch_arguments() {
        let content = prepare_app_content("com.test", 5);
        assert!(content.contains("--screenshot-"));
        assert!(content.contains("ProcessInfo"));
    }

    #[test]
    fn prepare_app_references_appshots_json() {
        let content = prepare_app_content("com.test", 5);
        assert!(content.contains("appshots.json"));
    }

    #[test]
    fn prepare_app_is_non_empty() {
        let content = prepare_app_content("x", 1);
        assert!(!content.is_empty());
        assert!(content.len() > 500, "prompt should be detailed");
    }

    // -- design-template ---------------------------------------------------

    #[test]
    fn design_template_contains_oklch() {
        let content = design_template_content("com.test", "dark minimal", false);
        assert!(content.contains("oklch"));
    }

    #[test]
    fn design_template_contains_measure() {
        let content = design_template_content("com.test", "dark", false);
        assert!(content.contains("measure"));
        assert!(content.contains("auto-text"));
    }

    #[test]
    fn design_template_contains_dont_section() {
        let content = design_template_content("com.test", "dark", false);
        assert!(content.contains("DON'T"));
    }

    #[test]
    fn design_template_contains_page_sizes() {
        let content = design_template_content("com.test", "dark", false);
        assert!(content.contains("440pt"));
        assert!(content.contains("956pt"));
        assert!(content.contains("1320"));
        assert!(content.contains("2868"));
    }

    #[test]
    fn design_template_references_rtl() {
        let content = design_template_content("com.test", "dark", false);
        assert!(content.contains("RTL"));
        assert!(content.contains("text_direction"));
    }

    #[test]
    fn design_template_references_cjk() {
        let content = design_template_content("com.test", "dark", false);
        assert!(content.contains("CJK"));
    }

    #[test]
    fn design_template_references_german_length() {
        let content = design_template_content("com.test", "dark", false);
        assert!(content.contains("German"));
        assert!(content.contains("30-40%"));
    }

    #[test]
    fn design_template_single_mode_note() {
        let content = design_template_content("com.test", "dark", false);
        assert!(content.contains("SINGLE shared template"));
        assert!(!content.contains("PER-SCREEN templates"));
    }

    #[test]
    fn design_template_per_screen_mode_note() {
        let content = design_template_content("com.test", "dark", true);
        assert!(content.contains("PER-SCREEN templates"));
        assert!(!content.contains("SINGLE shared template"));
    }

    #[test]
    fn design_template_contains_style() {
        let content = design_template_content("com.test", "vibrant gradients", false);
        assert!(content.contains("vibrant gradients"));
    }

    #[test]
    fn design_template_references_preview_tool() {
        let content = design_template_content("com.test", "dark", false);
        assert!(content.contains("preview_design"));
    }

    #[test]
    fn design_template_no_hex_colors_instruction() {
        let content = design_template_content("com.test", "dark", false);
        assert!(content.contains("No hex"));
    }

    #[test]
    fn design_template_wcag_contrast() {
        let content = design_template_content("com.test", "dark", false);
        assert!(content.contains("4.5:1"));
        assert!(content.contains("WCAG"));
    }

    #[test]
    fn design_template_contains_bundle_id() {
        let content = design_template_content("com.example.pro", "dark", false);
        assert!(content.contains("com.example.pro"));
    }

    // -- generate-screenshots ----------------------------------------------

    #[test]
    fn generate_screenshots_references_all_pipeline_tools() {
        let content = generate_screenshots_content("", "", "");
        let required_tools = [
            "scan_project",
            "analyze_keywords",
            "plan_screens",
            "save_captions",
            "get_locale_keywords",
            "validate_layout",
            "capture_screenshots",
            "compose_screenshots",
            "run_deliver",
        ];
        for tool in required_tools {
            assert!(
                content.contains(tool),
                "prompt must reference tool `{tool}`"
            );
        }
    }

    #[test]
    fn generate_screenshots_translation_rules() {
        let content = generate_screenshots_content("", "", "");
        assert!(content.contains("German"));
        assert!(content.contains("Japanese"));
        assert!(content.contains("Arabic"));
        assert!(content.contains("RTL"));
        assert!(content.contains("CJK") || content.contains("Chinese"));
    }

    #[test]
    fn generate_screenshots_uses_device_filter() {
        let content = generate_screenshots_content("iPhone 6.9\"", "", "");
        assert!(content.contains("devices: iPhone 6.9\""));
    }

    #[test]
    fn generate_screenshots_uses_locale_filter() {
        let content = generate_screenshots_content("", "en-US,de-DE", "");
        assert!(content.contains("locales: en-US,de-DE"));
    }

    #[test]
    fn generate_screenshots_uses_mode_filter() {
        let content = generate_screenshots_content("", "", "1,2,3");
        assert!(content.contains("modes: 1,2,3"));
    }

    #[test]
    fn generate_screenshots_defaults_all_when_empty() {
        let content = generate_screenshots_content("", "", "");
        assert!(content.contains("all configured devices"));
        assert!(content.contains("all 39 App Store locales"));
        assert!(content.contains("all configured screens"));
    }

    #[test]
    fn generate_screenshots_caption_quality_guidance() {
        let content = generate_screenshots_content("", "", "");
        assert!(content.contains("USER BENEFIT"));
        assert!(content.contains("GOOD") && content.contains("BAD"));
    }

    #[test]
    fn generate_screenshots_granular_regeneration() {
        let content = generate_screenshots_content("", "", "");
        assert!(content.contains("granular regeneration"));
    }

    #[test]
    fn generate_screenshots_is_non_empty() {
        let content = generate_screenshots_content("", "", "");
        assert!(!content.is_empty());
        assert!(
            content.len() > 1000,
            "pipeline prompt should be comprehensive"
        );
    }

    #[test]
    fn generate_screenshots_10_steps() {
        let content = generate_screenshots_content("", "", "");
        assert!(content.contains("Step 1"));
        assert!(content.contains("Step 10"));
    }
}
