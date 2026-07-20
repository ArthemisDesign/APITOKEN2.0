/**
 * Фильтр-DSL — единственный «жёсткий» слой системы (см. CRM_AI.md): AI придумывает фильтры,
 * этот модуль детерминированно их исполняет над картой признаков контакта.
 */

export type FilterOp = "eq" | "neq" | "in" | "contains" | "exists" | "gte" | "lte" | "regex";

export interface FilterCondition {
  key: string;
  op: FilterOp;
  value?: unknown;
}

export interface FilterDsl {
  all?: FilterCondition[];
  any?: FilterCondition[];
  none?: FilterCondition[];
}

export type AttributeMap = Record<string, unknown>;

const OPS: ReadonlySet<string> = new Set(["eq", "neq", "in", "contains", "exists", "gte", "lte", "regex"]);

/** Проверка формы DSL от AI: мусор отбрасываем ДО исполнения, с внятной ошибкой. */
export function parseFilter(input: unknown): FilterDsl {
  if (typeof input !== "object" || input === null || Array.isArray(input)) {
    throw new Error("filter must be an object");
  }
  const out: FilterDsl = {};
  for (const group of ["all", "any", "none"] as const) {
    const raw = (input as Record<string, unknown>)[group];
    if (raw === undefined) continue;
    if (!Array.isArray(raw)) throw new Error(`filter.${group} must be an array`);
    out[group] = raw.map((c, i) => {
      if (typeof c !== "object" || c === null) throw new Error(`filter.${group}[${i}] must be an object`);
      const { key, op, value } = c as Record<string, unknown>;
      if (typeof key !== "string" || key === "") throw new Error(`filter.${group}[${i}].key required`);
      if (typeof op !== "string" || !OPS.has(op)) throw new Error(`filter.${group}[${i}].op invalid`);
      if (op !== "exists" && value === undefined) throw new Error(`filter.${group}[${i}].value required`);
      return { key, op: op as FilterOp, value };
    });
  }
  return out;
}

function asComparable(v: unknown): number | null {
  if (typeof v === "number") return v;
  if (typeof v === "string" && v !== "" && !Number.isNaN(Number(v))) return Number(v);
  return null;
}

function scalarMatch(actual: unknown, op: FilterOp, expected: unknown): boolean {
  switch (op) {
    case "eq":
      return String(actual).toLowerCase() === String(expected).toLowerCase();
    case "neq":
      return String(actual).toLowerCase() !== String(expected).toLowerCase();
    case "in":
      return Array.isArray(expected) && expected.some((e) => scalarMatch(actual, "eq", e));
    case "contains":
      return String(actual).toLowerCase().includes(String(expected).toLowerCase());
    case "gte": {
      const a = asComparable(actual);
      const b = asComparable(expected);
      return a !== null && b !== null && a >= b;
    }
    case "lte": {
      const a = asComparable(actual);
      const b = asComparable(expected);
      return a !== null && b !== null && a <= b;
    }
    case "regex":
      try {
        return new RegExp(String(expected), "i").test(String(actual));
      } catch {
        return false;
      }
    case "exists":
      return true;
  }
}

function conditionMatches(attrs: AttributeMap, c: FilterCondition): boolean {
  const actual = attrs[c.key];
  if (actual === undefined || actual === null) return false;
  if (c.op === "exists") return true;
  // Массив признака (pain_points: [...]) матчится поэлементно; neq — против всех элементов.
  if (Array.isArray(actual)) {
    if (c.op === "neq") return actual.every((v) => scalarMatch(v, "neq", c.value));
    return actual.some((v) => scalarMatch(v, c.op, c.value));
  }
  return scalarMatch(actual, c.op, c.value);
}

export function matchesFilter(attrs: AttributeMap, filter: FilterDsl): boolean {
  if (filter.all && !filter.all.every((c) => conditionMatches(attrs, c))) return false;
  if (filter.any && filter.any.length > 0 && !filter.any.some((c) => conditionMatches(attrs, c))) return false;
  if (filter.none && filter.none.some((c) => conditionMatches(attrs, c))) return false;
  return true;
}
