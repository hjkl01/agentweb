#!/bin/sh
# 前端没有 @types/react，跑不了完整类型检查；这里只抓“引用了不存在的名字”
# 这类一进浏览器就白屏的错误（TS2304 等）。真正的类型检查要等装上 @types/react。
set -e
cd "$(dirname "$0")/.."
out=$(./node_modules/.bin/tsc --noEmit --jsx react-jsx --module esnext --target esnext \
  --moduleResolution bundler --skipLibCheck src/main.tsx 2>&1 \
  | grep -E "TS2304|TS2552|TS2306|TS2307" || true)
if [ -n "$out" ]; then
  echo "$out"
  echo "frontend: 引用了不存在的名字"
  exit 1
fi
echo "frontend names: ok"
