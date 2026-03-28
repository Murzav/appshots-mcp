// === appshots-mcp Reference Template ===
// Demonstrates device frame compositing with rounded corners,
// auto-scaling text, RTL support, and OKLCH gradient backgrounds.
//
// sys.inputs provided by appshots-mcp renderer:
//   caption_title    — main headline text (always present)
//   caption_subtitle — optional secondary text
//   keyword          — optional ASO keyword for reference
//   bg_color         — first background color as OKLCH string
//   bg_gradient      — comma-separated OKLCH colors for gradient
//   device_width     — target pixel width (e.g. "1320")
//   device_height    — target pixel height (e.g. "2868")
//   locale           — locale code (e.g. "en-US", "ar-SA")
//   text_direction   — "ltr" or "rtl"
//   screenshot_path  — "/screenshot.png" when capture exists, absent otherwise
//
// Page size: iPhone 6.9" at 1/3 scale (1320/3 = 440pt, 2868/3 = 956pt).
// The renderer upscales to full resolution automatically.

// --- Page setup ---
#set page(width: 440pt, height: 956pt, margin: 0pt)

// --- Read all sys.inputs with safe defaults ---
#let title = sys.inputs.at("caption_title", default: "Your App Title")
#let subtitle = sys.inputs.at("caption_subtitle", default: none)
#let keyword = sys.inputs.at("keyword", default: none)
#let bg-color = sys.inputs.at("bg_color", default: none)
#let bg-gradient-str = sys.inputs.at("bg_gradient", default: none)
#let device-w = sys.inputs.at("device_width", default: "1320")
#let device-h = sys.inputs.at("device_height", default: "2868")
#let locale = sys.inputs.at("locale", default: "en-US")
#let dir = sys.inputs.at("text_direction", default: "ltr")
#let screenshot = sys.inputs.at("screenshot_path", default: none)

// --- RTL support ---
// Flip horizontal alignment for RTL locales (ar-SA, he).
// text-direction is applied to the page so Typst reorders text automatically.
#let is-rtl = dir == "rtl"
#set text(dir: if is-rtl { rtl } else { ltr })

// --- OKLCH color palette ---
// All colors use OKLCH only (project requirement). No hex, RGB, or HSL.
#let bg-top = oklch(48%, 0.18, 265deg)
#let bg-bottom = oklch(35%, 0.20, 275deg)
#let white = oklch(99%, 0, 0deg)
#let dim-white = oklch(80%, 0, 0deg)

// --- Auto-text helper ---
// Scales text down until it fits within `max-width`.
// Uses `context` + `measure()` per Typst 0.14 API.
#let auto-text(body, max-size: 60pt, min-size: 28pt, step: 2pt, max-width: 400pt, weight: "bold") = {
  context {
    let sz = max-size
    while sz > min-size {
      let probe = text(size: sz, weight: weight)[#body]
      let m = measure(probe)
      if m.width <= max-width {
        // Found a size that fits — render and stop.
        text(size: sz, weight: weight, fill: white, tracking: -0.5pt)[#body]
        return
      }
      sz = sz - step
    }
    // Fallback to minimum size if nothing fit.
    text(size: min-size, weight: weight, fill: white, tracking: -0.5pt)[#body]
  }
}

// --- Background ---
// Gradient from top to bottom using OKLCH colors.
// If bg_gradient input is provided, real templates can parse the color string;
// here we use the palette defaults for simplicity.
#rect(
  width: 100%,
  height: 100%,
  fill: gradient.linear(bg-top, bg-bottom, angle: 180deg),
)[
  // ===== CAPTION AREA (top ~40%) =====
  // Title is centered (or end-aligned for RTL).
  #place(top + center, dy: 60pt)[
    #block(width: 380pt)[
      #set align(if is-rtl { right } else { center })
      #set par(leading: 0.75em)
      #auto-text(title, max-size: 62pt, min-size: 30pt, max-width: 370pt)
    ]
  ]

  // Optional subtitle below the title.
  #if subtitle != none {
    place(top + center, dy: 200pt)[
      #block(width: 380pt)[
        #set align(if is-rtl { right } else { center })
        #text(size: 24pt, weight: "medium", fill: dim-white)[#subtitle]
      ]
    ]
  }

  // ===== SCREENSHOT AREA (bottom ~60%) =====
  // Conditionally show the app screenshot when capture data is available.
  // The screenshot is clipped with rounded corners to create a device mockup
  // effect without needing an external device frame PNG.
  #if screenshot != none {
    place(bottom + center, dy: -40pt)[
      // Rounded-corner device mockup: box(clip: true) masks the image
      // to simulate the rounded display of modern iPhones/iPads.
      // radius: 34pt matches iPhone Pro display corner radius at 1/3 scale.
      #box(
        clip: true,
        radius: 34pt,
        width: 85%,
      )[
        #image(screenshot, width: 100%)
      ]
    ]
  } else {
    // Placeholder when no screenshot is available (design preview mode).
    // Shows a subtle card so the designer can see where the screenshot lands.
    place(bottom + center, dy: -40pt)[
      #box(
        width: 85%,
        height: 500pt,
        radius: 34pt,
        fill: oklch(25%, 0.05, 270deg),
        stroke: 1pt + oklch(35%, 0.08, 270deg),
      )[
        #set align(center + horizon)
        #text(size: 16pt, fill: oklch(55%, 0, 0deg))[
          Screenshot placeholder \
          #text(size: 12pt)[(capture will appear here)]
        ]
      ]
    ]
  }
]
