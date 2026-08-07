import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  evaluate,
  formatBases,
  parseInBase,
  type AngleMode,
  type Base,
} from "./engine";

const KEYS: { label: string; insert?: string; act?: "eq" | "clear" | "back" }[][] = [
  [
    { label: "sin", insert: "sin(" },
    { label: "cos", insert: "cos(" },
    { label: "tan", insert: "tan(" },
    { label: "π", insert: "π" },
    { label: "e", insert: "e" },
  ],
  [
    { label: "ln", insert: "ln(" },
    { label: "log", insert: "log(" },
    { label: "√", insert: "sqrt(" },
    { label: "x^y", insert: "^" },
    { label: "x!", insert: "!" },
  ],
  [
    { label: "(", insert: "(" },
    { label: ")", insert: ")" },
    { label: "%", insert: "%" },
    { label: "mod", insert: " mod " },
    { label: "C", act: "clear" },
  ],
  [
    { label: "7", insert: "7" },
    { label: "8", insert: "8" },
    { label: "9", insert: "9" },
    { label: "÷", insert: "/" },
    { label: "⌫", act: "back" },
  ],
  [
    { label: "4", insert: "4" },
    { label: "5", insert: "5" },
    { label: "6", insert: "6" },
    { label: "×", insert: "*" },
    { label: "^", insert: "^" },
  ],
  [
    { label: "1", insert: "1" },
    { label: "2", insert: "2" },
    { label: "3", insert: "3" },
    { label: "−", insert: "-" },
    { label: "+", insert: "+" },
  ],
  [
    { label: "0", insert: "0" },
    { label: ".", insert: "." },
    { label: "( )", insert: "()" },
    { label: "=", act: "eq" },
  ],
];

const BASES: { key: Base; label: string; base: number }[] = [
  { key: "dec", label: "DEC", base: 10 },
  { key: "hex", label: "HEX", base: 16 },
  { key: "oct", label: "OCT", base: 8 },
  { key: "bin", label: "BIN", base: 2 },
];

const EMPTY_BASES: Record<Base, string> = { dec: "", hex: "", oct: "", bin: "" };

function Calculator() {
  const { t } = useTranslation();
  const [expr, setExpr] = useState("");
  const [angle, setAngle] = useState<AngleMode>("rad");
  const [history, setHistory] = useState<{ expr: string; result: string }[]>([]);

  const preview = useMemo(() => {
    if (!expr.trim()) return { value: null as number | null, error: null as string | null };
    try {
      return { value: evaluate(expr, angle), error: null };
    } catch (e) {
      return { value: null, error: e instanceof Error ? e.message : t("tool.calc.error") };
    }
  }, [expr, angle]);

  const press = (k: (typeof KEYS)[number][number]) => {
    if (k.act === "clear") return setExpr("");
    if (k.act === "back") return setExpr((e) => e.slice(0, -1));
    if (k.act === "eq") return commit();
    if (k.insert === "()") return setExpr((e) => e + "()");
    if (k.insert) setExpr((e) => e + k.insert);
  };

  const commit = () => {
    if (preview.value == null) return;
    const result = fmtNum(preview.value);
    setHistory((h) => [{ expr, result }, ...h].slice(0, 20));
    setExpr(result);
  };

  return (
    <div className="calc">
      <div className="calc-display">
        <input
          className="calc-expr"
          value={expr}
          onChange={(e) => setExpr(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              commit();
            }
          }}
          placeholder={t("tool.calc.exprPlaceholder")}
          spellCheck={false}
          autoFocus
        />
        <div className={`calc-result ${preview.error ? "err" : ""}`}>
          {preview.error ? preview.error : preview.value != null ? `= ${fmtNum(preview.value)}` : " "}
        </div>
      </div>

      <div className="calc-modebar">
        <div className="segmented">
          {(["rad", "deg"] as AngleMode[]).map((m) => (
            <button key={m} className={angle === m ? "active" : ""} onClick={() => setAngle(m)}>
              {m.toUpperCase()}
            </button>
          ))}
        </div>
        <span className="dim">
          {t("tool.calc.trigIn", {
            mode: angle === "deg" ? t("tool.calc.degrees") : t("tool.calc.radians"),
          })}
        </span>
      </div>

      <div className="calc-keys">
        {KEYS.map((row, i) => (
          <div key={i} className="calc-row">
            {row.map((k) => (
              <button
                key={k.label}
                className={`calc-key ${k.act === "eq" ? "primary" : ""} ${
                  k.act === "clear" || k.act === "back" ? "warn" : ""
                }`}
                onClick={() => press(k)}
              >
                {k.label}
              </button>
            ))}
          </div>
        ))}
      </div>

      {history.length > 0 && (
        <div className="calc-history">
          <div className="section-header">
            <h4>{t("tool.calc.history")}</h4>
            <button className="small ghost" onClick={() => setHistory([])}>
              {t("tool.calc.clear")}
            </button>
          </div>
          <ul>
            {history.map((h, i) => (
              <li key={i}>
                <button className="calc-hist-item" onClick={() => setExpr(h.expr)}>
                  <span className="dim">{h.expr}</span> = <strong>{h.result}</strong>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

function fmtNum(n: number): string {
  if (!Number.isFinite(n)) return n > 0 ? "∞" : "-∞";
  if (Number.isInteger(n) && Math.abs(n) < 1e15) return n.toString();
  return parseFloat(n.toPrecision(12)).toString();
}

function BaseConverter() {
  const { t } = useTranslation();
  const [fields, setFields] = useState<Record<Base, string>>(EMPTY_BASES);
  const [error, setError] = useState<string | null>(null);

  const edit = (base: Base, text: string) => {
    if (text.trim() === "") {
      setFields(EMPTY_BASES);
      setError(null);
      return;
    }
    const value = parseInBase(text, base);
    if (value == null) {
      setFields((f) => ({ ...f, [base]: text }));
      setError(t("tool.calc.invalidInBase", { text, base: base.toUpperCase() }));
      return;
    }
    setError(null);
    setFields({ ...formatBases(value), [base]: text });
  };

  return (
    <div className="base-conv">
      <h4>{t("tool.calc.baseConverter")}</h4>
      <div className="base-grid">
        {BASES.map((b) => (
          <label key={b.key} className="base-field">
            <span className="base-label">
              {b.label} <span className="dim">{t("tool.calc.baseHint", { n: b.base })}</span>
            </span>
            <input
              value={fields[b.key]}
              onChange={(e) => edit(b.key, e.target.value)}
              placeholder="0"
              spellCheck={false}
              inputMode={b.key === "dec" ? "numeric" : "text"}
            />
          </label>
        ))}
      </div>
      {error && <div className="banner banner-error">{error}</div>}
      <p className="hint">{t("tool.calc.bigIntHint")}</p>
    </div>
  );
}

export function Calc() {
  const { t } = useTranslation();
  return (
    <div>
      <div className="section-header">
        <h2>{t("tool.calc.title")}</h2>
      </div>
      <div className="calc-layout">
        <Calculator />
        <BaseConverter />
      </div>
    </div>
  );
}
