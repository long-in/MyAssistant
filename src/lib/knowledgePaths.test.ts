import { describe, expect, it } from "vitest";
import { isReadOnlyPath } from "./knowledgePaths";

describe("isReadOnlyPath", () => {
  it("ナレッジ直下の data/ の配下は閲覧専用", () => {
    expect(isReadOnlyPath("data")).toBe(true);
    expect(isReadOnlyPath("data/app/main.rs")).toBe(true);
    expect(isReadOnlyPath("DATA/README.md")).toBe(true);
  });
  it(". 始まりを含むパスは閲覧専用", () => {
    expect(isReadOnlyPath(".my-assistant/command-policy.json")).toBe(true);
    expect(isReadOnlyPath("debug-logs/.hidden")).toBe(true);
  });
  it("ほかは編集できる(直下以外の data は対象外)", () => {
    expect(isReadOnlyPath("dev/note.md")).toBe(false);
    expect(isReadOnlyPath("dev/data/x.md")).toBe(false);
    expect(isReadOnlyPath("database/a.md")).toBe(false);
  });
});
