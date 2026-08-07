import { useMemo, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { post } from "../../lib/api";
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

const HAS_EYEDROPPER = typeof window !== "undefined" && "EyeDropper" in window;
const IS_TAURI = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
const IS_MAC = typeof navigator !== "undefined" && /Macintosh/.test(navigator.userAgent);
const USE_COLOR_METER = !HAS_EYEDROPPER && IS_TAURI && IS_MAC;

function hsvaToRgba({ h, s, v, a }: HSVA): RGBA {
  return { ...hsvToRgb(h, s, v), a };
}

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
  const { t } = useTranslation();
  const [hsva, setHsva] = useState<HSVA>({ h: 210, s: 75, v: 90, a: 1 });
  const [input, setInput] = useState("");
  const [inputErr, setInputErr] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const [meterMsg, setMeterMsg] = useState<string | null>(null);

  const rgba = useMemo(() => hsvaToRgba(hsva), [hsva]);
  const opaqueHex = toHex({ ...rgba, a: 1 });

  const sv = useDrag((x, y) => setHsva((c) => ({ ...c, s: x * 100, v: (1 - y) * 100 })));
  const hueBar = useDrag((x) => setHsva((c) => ({ ...c, h: x * 360 })));
  const alphaBar = useDrag((x) => setHsva((c) => ({ ...c, a: x })));

  const setFromRgba = (c: RGBA) => {
    const { h, s, v } = rgbToHsv(c.r, c.g, c.b);
    setHsva({ h, s, v, a: c.a });
  };

  const applyInput = (text: string) => {
    setInput(text);
    const parsed = parseColor(text);
    if (parsed) {
      setFromRgba(parsed);
      setInputErr(false);
    } else {
      setInputErr(true);
    }
  };

  const applyLoose = (text: string): boolean => {
    const trimmed = text.trim();
    const parsed = parseColor(trimmed);
    if (parsed) {
      setFromRgba(parsed);
      return true;
    }
    const nums = (trimmed.match(/\d+(?:\.\d+)?/g) ?? []).map(Number);
    if (nums.length >= 3 && nums.slice(0, 3).every((n) => n <= 255)) {
      const [r, g, b, a] = nums;
      setFromRgba({
        r: Math.round(r),
        g: Math.round(g),
        b: Math.round(b),
        a: a != null && a <= 1 ? a : 1,
      });
      return true;
    }
    return false;
  };

  const openColorMeter = () => post("/api/system/color-meter", {});

  const readFromClipboard = async () => {
    setMeterMsg(null);
    let text = "";
    try {
      text = await navigator.clipboard.readText();
    } catch {
      setMeterMsg(t("tool.color.clipReadError"));
      return;
    }
    if (applyLoose(text)) {
      setInput(text.trim());
      setInputErr(false);
    } else {
      setMeterMsg(t("tool.color.clipNoColor"));
    }
  };

  const copy = async (value: string) => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(value);
      setTimeout(() => setCopied(null), 1200);
    } catch {
    }
  };

  const pickScreen = async () => {
    try {
      // @ts-expect-error EyeDropper non è ancora nei lib DOM di TS
      const result = await new window.EyeDropper().open();
      applyInput(result.sRGBHex);
    } catch {
    }
  };

  return (
    <div>
      <div className="section-header">
        <h2>{t("tool.color.title")}</h2>
        {HAS_EYEDROPPER ? (
          <button className="small" onClick={pickScreen} title={t("tool.color.pickScreenTitle")}>
            {t("tool.color.pickScreen")}
          </button>
        ) : USE_COLOR_METER ? (
          <div className="color-meter-actions">
            <button className="small" onClick={openColorMeter} title={t("tool.color.colorMeterTitle")}>
              {t("tool.color.colorMeter")}
            </button>
            <button
              className="small ghost"
              onClick={readFromClipboard}
              title={t("tool.color.readClipboardTitle")}
            >
              {t("tool.color.readClipboard")}
            </button>
          </div>
        ) : (
          <span className="dim" title={t("tool.color.notSupportedTitle")}>
            {t("tool.color.notSupported")}
          </span>
        )}
      </div>

      {USE_COLOR_METER && (
        <div className="color-meter-hint hint">
          <Trans
            i18nKey="tool.color.meterHint"
            components={{ b: <strong />, kbd: <kbd />, em: <em /> }}
          />
          {meterMsg && <span className="banner-error-text"> {meterMsg}</span>}
        </div>
      )}

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
            <span className="dim">{t("tool.color.pasteColor")}</span>
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
                    {copied === value ? "✓" : t("tool.color.copy")}
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
