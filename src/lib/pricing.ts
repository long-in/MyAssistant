/** 100万トークンあたりの概算単価(USD)。公開価格に基づく目安で、請求額とは一致しない場合がある。 */
export const PRICES: Record<string, { input: number; output: number }> = {
  "claude-haiku-5-5": { input: 0.1, output: 0.5 },
  "claude-sonnet-5-5": { input: 2, output: 10 },
  "claude-opus-5-5": { input: 4, output: 20 },
  "gpt-4o-mini": { input: 0.15, output: 0.6 },
  "gpt-4o": { input: 2.5, output: 10 },
};

/** 推定コスト(USD)。単価が未登録のモデルは null。 */
export function estimateCost(model: string, input: number, output: number): number | null {
  const p = PRICES[model];
  return p ? (input * p.input + output * p.output) / 1_000_000 : null;
}

export const formatUsd = (v: number) => (v < 0.01 ? `$${v.toFixed(4)}` : `$${v.toFixed(2)}`);
