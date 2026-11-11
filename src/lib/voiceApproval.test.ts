import { describe, expect, it } from "vitest";
import { judgeAnswer, voiceCanApprove } from "./voiceApproval";

describe("judgeAnswer", () => {
  it("はっきりした肯定だけを承認にする", () => {
    for (const s of ["はい", "はい、お願いします", "OK", "実行して", "大丈夫です", "問題ないです", "構いません", "はい、問題ありません"]) {
      expect(judgeAnswer(s), s).toBe("yes");
    }
  });

  it("否定の語と、肯定の語を打ち消す言い方は拒否にする", () => {
    for (const s of [
      "いいえ", "やめて", "はい、でもやめて", "許可しない", "許可しません", "実行しない", "実行しません", "承認できない",
      "大丈夫じゃない", "進めないで", "まだ実行しないで", "それは困る", "あとでお願い", "don't", "not ok",
    ]) {
      expect(judgeAnswer(s), s).toBe("no");
    }
  });

  it("どちらでもない発話と長い発話は、返答として扱わない", () => {
    expect(judgeAnswer("")).toBe("unclear");
    expect(judgeAnswer("今日の天気")).toBe("unclear");
    expect(judgeAnswer("はい".repeat(20))).toBe("unclear");
  });
});

describe("voiceCanApprove", () => {
  it("コマンドの個別承認と許可リストへの追加は、音声では承認できない", () => {
    expect(voiceCanApprove("command")).toBe(false);
    expect(voiceCanApprove("allow_command")).toBe(false);
    for (const kind of ["plan", "file", "skill_update", "skill_delete"]) expect(voiceCanApprove(kind), kind).toBe(true);
  });
});
