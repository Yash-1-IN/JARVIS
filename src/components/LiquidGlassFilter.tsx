/**
 * Presentation-only SVG filter for Liquid Glass refraction (#156).
 *
 * Provides `#glass-refraction` for `backdrop-filter: url(#glass-refraction)`
 * on supported engines (e.g. WebView2 on Windows).
 * Edge refraction bends the backdrop gently while keeping the centre calm
 * and text legible.
 */
export function LiquidGlassFilter() {
  return (
    <svg
      aria-hidden="true"
      className="visually-hidden"
      style={{
        position: "absolute",
        width: 0,
        height: 0,
        overflow: "hidden",
        pointerEvents: "none",
      }}
    >
      <defs>
        <filter
          id="glass-refraction"
          x="-10%"
          y="-10%"
          width="120%"
          height="120%"
          colorInterpolationFilters="sRGB"
        >
          <feTurbulence
            type="fractalNoise"
            baseFrequency="0.035"
            numOctaves={2}
            result="noise"
          />
          <feDisplacementMap
            in="SourceGraphic"
            in2="noise"
            scale="4"
            xChannelSelector="R"
            yChannelSelector="G"
            result="displaced"
          />
          <feGaussianBlur in="displaced" stdDeviation="1.2" result="blurred" />
          <feBlend in="SourceGraphic" in2="blurred" mode="normal" />
        </filter>
      </defs>
    </svg>
  );
}
