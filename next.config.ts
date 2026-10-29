import type { NextConfig } from "next";
import pkg from "./package.json";

const nextConfig: NextConfig = {
  output: "export",
  // アプリバージョンの単一の情報源はpackage.json。フロントへはビルド時に埋め込む
  env: { NEXT_PUBLIC_APP_VERSION: pkg.version },
  images: { unoptimized: true },
};

export default nextConfig;
