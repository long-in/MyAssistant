import { describe, expect, it } from "vitest";
import { detectSwitch } from "./voiceCommands";

describe("detectSwitch", () => {
  it("切り替えの指示だけの発話", () => {
    for (const s of ["新しい会話にして", "話題を変えてください", "リセット"]) expect(detectSwitch(s)?.kind, s).toBe("command");
  });

  it("切り替えの表現に続けて依頼がある発話", () => {
    expect(detectSwitch("話は変わるけど、明日の予定を教えて")?.kind).toBe("mixed");
  });

  it("関係のない発話", () => {
    expect(detectSwitch("今日の天気を教えて")).toBeNull();
    expect(detectSwitch("")).toBeNull();
  });
});
