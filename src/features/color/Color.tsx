import { useMemo, useRef, useState } from "react";
import {
  hsvToRgb,
  parseColor,
  rgbToHsv,
  toHex,
  toHslString,
  toRgbString,
  toRgbaString,
  type RGBA,
} from "./convert";

interface HSVA {
  h: number;
  s: number;
  v: number;
  a: number;
}

// L'API EyeDropper esiste solo su webview Chromium (Windows WebView2), non su
// WKWebView (macOS). Feature-detection: il bottone appare solo se usabile.
const HAS_EYEDROPPER = typeof window !== "undefined" && "EyeDropper" in window;

function hsvaToRgba({ h, s, v, a }: HSVA): RGBA {
  return { ...hsvToRgb(h, s, v), a };
}

/** Handler di trascinamento su un elemento: riporta x,y normalizzati 0..1. */
function useDrag(onMove: (x: number, y: number) => void) {
  const ref = useRef<HTMLDivElement>(null);
  const move = (e: PointerEvent | React.PointerEvent) => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const x = Math.min(1, Math.max(0, ((e as PointerEvent).clientX - rect.left) / rect.width));
    const y = Math.min(1, Math.max(0, ((e as PointerEvent).clientY - rect.top) / rect.height));
    onMove(x, y);
  };
  const onPointerDown = (e: React.PointerEvent) => {
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    move(e);
    const up = () => {
      window.removeEventListener("pointermove", move as EventListener);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move as EventListener);
    window.addEventListener("pointerup", up);
  };
  return { ref, onPointerDown };
}

const FORMATS = [
  { key: "hex", label: "HEX", fmt: (c: RGBA) => toHex(c, c.a < 1) },
  { key: "rgb", label: "RGB", fmt: (c: RGBA) => toRgbString(c) },
  { key: "rgba", label: "RGBA", fmt: (c: RGBA) => toRgbaString(c) },
  { key: "hsl", label: "HSL", fmt: (c: RGBA) => toHslString(c) },
] as const;

export function Color() {
  const [hsva, setHsva] = useState<HSVA>({ h: 210, s: 75, v: 90, a: 1 });
  const [input, setInput] = useState("");
  const [inputErr, setInputErr] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);

  const rgba = useMemo(() => hsvaToRgba(hsva), [hsva]);
  const opaqueHex = toHex({ ...rgba, a: 1 });

  const sv = useDrag((x, y) => setHsva((c) => ({ ...c, s: x * 100, v: (1 - y) * 100 })));
  const hueBar = useDrag((x) => setHsva((c) => ({ ...c, h: x * 360 })));
  const alphaBar = useDrag((x) => setHsva((c) => ({ ...c, a: x })));

  const applyInput = (text: string) => {
    setInput(text);
    const parsed = parseColor(text);
    if (parsed) {
      const { h, s, v } = rgbToHsv(parsed.r, parsed.g, parsed.b);
      setHsva({ h, s, v, a: parsed.a });
      setInputErr(false);
    } else {
      setInputErr(true);
    }
  };

  const copy = async (value: string) => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(value);
      setTimeout(() => setCopied(null), 1200);
    } catch {
      /* clipboard negata: ignora */
    }
  };

  const pickScreen = async () => {
    try {
      // @ts-expect-error EyeDropper non è ancora nei lib DOM di TS
      const result = await new window.EyeDropper().open();
      applyInput(result.sRGBHex);
    } catch {
      /* annullato dall'utente */
    }
  };

  return (
    <div>
      <div className="section-header">
        <h2>Color picker</h2>
        {HAS_EYEDROPPER ? (
          <button className="small" onClick={pickScreen} title="Preleva un colore dallo schermo">
            🎯 Preleva dallo schermo
          </button>
        ) : (
          <span className="dim" title="Disponibile su Windows; WKWebView (macOS) non espone l'API">
            eyedropper non supportato qui
          </span>
        )}
      </div>

      <div className="color-layout">
        <div className="color-pickers">
          {/* Quadrato saturazione/valore */}
          <div
            className="color-sv"
            ref={sv.ref}
            onPointerDown={sv.onPointerDown}
            style={{ background: `hsl(${hsva.h}, 100%, 50%)` }}
          >
            <div className="color-sv-white" />
            <div className="color-sv-black" />
            <div
              className="color-thumb"
              style={{
                left: `${hsva.s}%`,
                top: `${100 - hsva.v}%`,
                background: opaqueHex,
              }}
            />
          </div>

          {/* Slider tinta */}
          <div className="color-hue" ref={hueBar.ref} onPointerDown={hueBar.onPointerDown}>
            <div
              className="color-slider-thumb"
              style={{ left: `${(hsva.h / 360) * 100}%`, background: `hsl(${hsva.h},100%,50%)` }}
            />
          </div>

          {/* Slider alpha su scacchiera */}
          <div className="color-alpha" ref={alphaBar.ref} onPointerDown={alphaBar.onPointerDown}>
            <div
              className="color-alpha-fill"
              style={{ background: `linear-gradient(to right, transparent, ${opaqueHex})` }}
            />
            <div
              className="color-slider-thumb"
              style={{ left: `${hsva.a * 100}%`, background: opaqueHex }}
            />
          </div>
        </div>

        <div className="color-side">
          <div className="color-preview-wrap">
            <div className="color-preview" style={{ background: toRgbaString(rgba) }} />
            <div className="color-preview-meta dim">
              {rgba.r}, {rgba.g}, {rgba.b} · α {rgba.a.toFixed(2)}
            </div>
          </div>

          <label className="color-input-row">
            <span className="dim">Incolla un colore</span>
            <input
              className={inputErr ? "err" : ""}
              value={input}
              onChange={(e) => applyInput(e.target.value)}
              placeholder="#3aa0e6 · rgb(58,160,230) · hsl(205,78%,56%)"
              spellCheck={false}
            />
          </label>

          <div className="color-formats">
            {FORMATS.map((f) => {
              const value = f.fmt(rgba);
              return (
                <div key={f.key} className="color-format">
                  <span className="color-format-label">{f.label}</span>
                  <code className="color-format-value">{value}</code>
                  <button className="small ghost" onClick={() => copy(value)}>
                    {copied === value ? "✓" : "copia"}
                  </button>
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
