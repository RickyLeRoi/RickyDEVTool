export type AngleMode = "deg" | "rad";

const CONSTS: Record<string, number> = {
  pi: Math.PI,
  "π": Math.PI,
  e: Math.E,
  tau: Math.PI * 2,
};

type FnDef = (x: number) => number;

function makeFns(angle: AngleMode): Record<string, FnDef> {
  const toRad = (x: number) => (angle === "deg" ? (x * Math.PI) / 180 : x);
  const fromRad = (x: number) => (angle === "deg" ? (x * 180) / Math.PI : x);
  return {
    sin: (x) => Math.sin(toRad(x)),
    cos: (x) => Math.cos(toRad(x)),
    tan: (x) => Math.tan(toRad(x)),
    asin: (x) => fromRad(Math.asin(x)),
    acos: (x) => fromRad(Math.acos(x)),
    atan: (x) => fromRad(Math.atan(x)),
    sinh: Math.sinh,
    cosh: Math.cosh,
    tanh: Math.tanh,
    ln: Math.log,
    log: Math.log10,
    log2: Math.log2,
    sqrt: Math.sqrt,
    cbrt: Math.cbrt,
    exp: Math.exp,
    abs: Math.abs,
    round: Math.round,
    floor: Math.floor,
    ceil: Math.ceil,
  };
}

type Token =
  | { t: "num"; v: number }
  | { t: "op"; v: string }
  | { t: "lparen" }
  | { t: "rparen" }
  | { t: "ident"; v: string };

function tokenize(input: string): Token[] {
  const tokens: Token[] = [];
  let i = 0;
  const s = input;
  while (i < s.length) {
    const c = s[i];
    if (c === " " || c === "\t" || c === "\n") {
      i++;
      continue;
    }
    if (c >= "0" && c <= "9") {
      let j = i;
      while (j < s.length && /[0-9._]/.test(s[j])) j++;
      if (s[j] === "e" || s[j] === "E") {
        if (s[j + 1] === "+" || s[j + 1] === "-" || /[0-9]/.test(s[j + 1] ?? "")) {
          j++;
          if (s[j] === "+" || s[j] === "-") j++;
          while (j < s.length && /[0-9]/.test(s[j])) j++;
        }
      }
      const raw = s.slice(i, j).replace(/_/g, "");
      const v = Number(raw);
      if (!Number.isFinite(v)) throw new Error(`numero non valido: ${raw}`);
      tokens.push({ t: "num", v });
      i = j;
      continue;
    }
    if (/[a-zA-Zπ]/.test(c)) {
      let j = i;
      while (j < s.length && /[a-zA-Z0-9π]/.test(s[j])) j++;
      tokens.push({ t: "ident", v: s.slice(i, j) });
      i = j;
      continue;
    }
    if (c === "(") {
      tokens.push({ t: "lparen" });
      i++;
      continue;
    }
    if (c === ")") {
      tokens.push({ t: "rparen" });
      i++;
      continue;
    }
    if ("+-*/^%!×÷".includes(c)) {
      const v = c === "×" ? "*" : c === "÷" ? "/" : c;
      tokens.push({ t: "op", v });
      i++;
      continue;
    }
    throw new Error(`carattere non riconosciuto: "${c}"`);
  }
  return tokens;
}

function factorial(n: number): number {
  if (n < 0 || !Number.isInteger(n)) throw new Error("fattoriale definito solo su interi ≥ 0");
  if (n > 170) return Infinity;
  let r = 1;
  for (let k = 2; k <= n; k++) r *= k;
  return r;
}

class Parser {
  private pos = 0;
  constructor(
    private tokens: Token[],
    private fns: Record<string, FnDef>,
  ) {}

  private peek(): Token | undefined {
    return this.tokens[this.pos];
  }
  private next(): Token | undefined {
    return this.tokens[this.pos++];
  }

  parse(): number {
    const v = this.expr();
    if (this.pos < this.tokens.length) throw new Error("espressione malformata");
    return v;
  }

  private expr(): number {
    let v = this.term();
    for (;;) {
      const tk = this.peek();
      if (tk?.t === "op" && (tk.v === "+" || tk.v === "-")) {
        this.next();
        const rhs = this.term();
        v = tk.v === "+" ? v + rhs : v - rhs;
      } else break;
    }
    return v;
  }

  private term(): number {
    let v = this.unary();
    for (;;) {
      const tk = this.peek();
      if (tk?.t === "op" && (tk.v === "*" || tk.v === "/" || tk.v === "%")) {
        this.next();
        const rhs = this.unary();
        if (tk.v === "*") v = v * rhs;
        else if (tk.v === "/") v = v / rhs;
        else v = v % rhs;
      } else if (tk?.t === "ident" && tk.v === "mod") {
        this.next();
        v = v % this.unary();
      } else break;
    }
    return v;
  }

  private unary(): number {
    const tk = this.peek();
    if (tk?.t === "op" && (tk.v === "+" || tk.v === "-")) {
      this.next();
      const v = this.unary();
      return tk.v === "-" ? -v : v;
    }
    return this.power();
  }

  private power(): number {
    const base = this.postfix();
    const tk = this.peek();
    if (tk?.t === "op" && tk.v === "^") {
      this.next();
      return Math.pow(base, this.unary());
    }
    return base;
  }

  private postfix(): number {
    let v = this.primary();
    for (;;) {
      const tk = this.peek();
      if (tk?.t === "op" && tk.v === "!") {
        this.next();
        v = factorial(v);
      } else break;
    }
    return v;
  }

  private primary(): number {
    const tk = this.next();
    if (!tk) throw new Error("espressione incompleta");
    if (tk.t === "num") return tk.v;
    if (tk.t === "lparen") {
      const v = this.expr();
      const close = this.next();
      if (close?.t !== "rparen") throw new Error("parentesi ) mancante");
      return v;
    }
    if (tk.t === "ident") {
      const name = tk.v.toLowerCase();
      if (this.peek()?.t === "lparen") {
        const fn = this.fns[name];
        if (!fn) throw new Error(`funzione sconosciuta: ${tk.v}`);
        this.next();
        const arg = this.expr();
        const close = this.next();
        if (close?.t !== "rparen") throw new Error("parentesi ) mancante");
        return fn(arg);
      }
      const c = CONSTS[name] ?? CONSTS[tk.v];
      if (c !== undefined) return c;
      throw new Error(`identificatore sconosciuto: ${tk.v}`);
    }
    throw new Error("token inatteso");
  }
}

export function evaluate(expr: string, angle: AngleMode = "rad"): number {
  const trimmed = expr.trim();
  if (!trimmed) throw new Error("espressione vuota");
  const tokens = tokenize(trimmed);
  const result = new Parser(tokens, makeFns(angle)).parse();
  if (Number.isNaN(result)) throw new Error("risultato non definito");
  return result;
}

export type Base = "dec" | "hex" | "oct" | "bin";

const RADIX: Record<Base, number> = { dec: 10, hex: 16, oct: 8, bin: 2 };

export function parseInBase(text: string, base: Base): bigint | null {
  let s = text.trim().toLowerCase().replace(/[_\s]/g, "");
  if (!s || s === "-" || s === "+") return null;
  let neg = false;
  if (s.startsWith("-")) {
    neg = true;
    s = s.slice(1);
  } else if (s.startsWith("+")) {
    s = s.slice(1);
  }
  const prefixes: Record<string, Base> = { "0x": "hex", "0o": "oct", "0b": "bin" };
  const pfx = s.slice(0, 2);
  if (prefixes[pfx] === base) s = s.slice(2);

  const radix = RADIX[base];
  const digits = "0123456789abcdef".slice(0, radix);
  let acc = 0n;
  const r = BigInt(radix);
  for (const ch of s) {
    const d = digits.indexOf(ch);
    if (d < 0) return null;
    acc = acc * r + BigInt(d);
  }
  return neg ? -acc : acc;
}

export function formatBases(value: bigint): Record<Base, string> {
  const neg = value < 0n;
  const abs = neg ? -value : value;
  const sign = neg ? "-" : "";
  return {
    dec: sign + abs.toString(10),
    hex: sign + abs.toString(16).toUpperCase(),
    oct: sign + abs.toString(8),
    bin: sign + abs.toString(2),
  };
}
