export interface TextLine {
  kind: "same" | "add" | "remove";
  text: string;
  before?: number;
  after?: number;
}
const lines = (text: string) => {
  const values = text.split("\n");
  if (values.at(-1) === "") values.pop();
  return values;
};
/** A bounded LCS diff. Large changed regions remain valid delete/add blocks. */
export function textDiff(before: string, after: string): TextLine[] {
  const a = lines(before),
    b = lines(after),
    out: TextLine[] = [];
  let prefix = 0,
    suffix = 0;
  while (prefix < a.length && prefix < b.length && a[prefix] === b[prefix])
    prefix++;
  while (
    suffix < a.length - prefix &&
    suffix < b.length - prefix &&
    a[a.length - suffix - 1] === b[b.length - suffix - 1]
  )
    suffix++;
  for (let i = 0; i < prefix; i++)
    out.push({ kind: "same", text: a[i], before: i + 1, after: i + 1 });
  const n = a.length - prefix - suffix,
    m = b.length - prefix - suffix;
  if (n * m <= 4_000_000) {
    const rows = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
    for (let i = n - 1; i >= 0; i--)
      for (let j = m - 1; j >= 0; j--)
        rows[i][j] =
          a[prefix + i] === b[prefix + j]
            ? rows[i + 1][j + 1] + 1
            : Math.max(rows[i + 1][j], rows[i][j + 1]);
    let i = 0,
      j = 0;
    while (i < n || j < m) {
      if (i < n && j < m && a[prefix + i] === b[prefix + j]) {
        out.push({
          kind: "same",
          text: a[prefix + i],
          before: prefix + i + 1,
          after: prefix + j + 1,
        });
        i++;
        j++;
      } else if (i < n && (j === m || rows[i + 1][j] >= rows[i][j + 1])) {
        out.push({
          kind: "remove",
          text: a[prefix + i],
          before: prefix + i + 1,
        });
        i++;
      } else {
        out.push({ kind: "add", text: b[prefix + j], after: prefix + j + 1 });
        j++;
      }
    }
  } else {
    for (let i = prefix; i < a.length - suffix; i++)
      out.push({ kind: "remove", text: a[i], before: i + 1 });
    for (let i = prefix; i < b.length - suffix; i++)
      out.push({ kind: "add", text: b[i], after: i + 1 });
  }
  for (let i = suffix; i > 0; i--)
    out.push({
      kind: "same",
      text: a[a.length - i],
      before: a.length - i + 1,
      after: b.length - i + 1,
    });
  return out;
}
