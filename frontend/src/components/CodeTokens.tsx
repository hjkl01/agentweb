import type { ReactNode } from 'react';

type Token = { text: string; className?: string };

const keywords = new Set('as async await break case catch class const continue crate default defer delete else enum export extends false fn for from function if implements import in interface let match mod new null of package private protected pub return self static struct super switch this throw trait true try type typeof use var while where with yield'.split(' '));
const types = new Set('bool char f32 f64 i8 i16 i32 i64 isize str String u8 u16 u32 u64 usize number string boolean void any unknown never'.split(' '));

function tokenize(line: string): Token[] {
  const result: Token[] = [];
  const pattern = /(\/\/.*|#.*|\/\*.*?\*\/|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`|\b\d+(?:\.\d+)?\b|\b[A-Za-z_$][\w$]*\b)/g;
  let last = 0;
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(line))) {
    if (match.index > last) result.push({ text: line.slice(last, match.index) });
    const value = match[0];
    let className: string | undefined;
    if (/^(\/\/|#|\/\*)/.test(value)) className = 'token-comment';
    else if (/^["'`]/.test(value)) className = 'token-string';
    else if (/^\d/.test(value)) className = 'token-number';
    else if (keywords.has(value)) className = 'token-keyword';
    else if (types.has(value)) className = 'token-type';
    result.push({ text: value, className });
    last = match.index + value.length;
  }
  if (last < line.length) result.push({ text: line.slice(last) });
  return result;
}

export function CodeTokens({ line }: { line: string }): ReactNode {
  return <>{tokenize(line).map((token, index) => <span className={token.className} key={`${index}-${token.text}`}>{token.text}</span>)}</>;
}
