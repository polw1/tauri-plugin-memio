import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const appDir = resolve(__dirname, "..");

function run(cmd, args, options = {}) {
  execFileSync(cmd, args, { stdio: "inherit", ...options });
}

try {
  run("adb", ["reverse", "tcp:1420", "tcp:1420"]);
  run("adb", ["reverse", "tcp:1421", "tcp:1421"]);
} catch (error) {
  console.warn("[android] adb reverse failed, continuing...", error?.message ?? error);
}

const env = {
  ...process.env,
  TAURI_DEV_HOST: "127.0.0.1",
};

run("npx", ["tauri", "android", "dev", "--host", "127.0.0.1"], { cwd: appDir, env });
