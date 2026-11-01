import { mkdir, copyFile, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import assert from "node:assert/strict";

const root = resolve(import.meta.dir, "..");
const build = Bun.spawnSync([
  "cargo", "build", "--locked", "--release", "--target", "x86_64-unknown-uefi", "--target-dir", resolve(root, "target"),
], { cwd: root, stdout: "inherit", stderr: "inherit" });
if (build.exitCode !== 0) process.exit(build.exitCode || 1);

await mkdir(resolve(root, "dist/EFI/BOOT"), { recursive: true });
await copyFile(
  resolve(root, "target/x86_64-unknown-uefi/release/tbt-retry.efi"),
  resolve(root, "dist/EFI/BOOT/BOOTX64.EFI"),
);
const image = await readFile(resolve(root, "dist/EFI/BOOT/BOOTX64.EFI"));
assert.equal(image.toString("ascii", 0, 2), "MZ");
const pe = image.readUInt32LE(0x3c);
assert.equal(image.readUInt32LE(pe), 0x00004550, "Missing PE signature");
assert.equal(image.readUInt16LE(pe + 4), 0x8664, "Expected x86_64");
assert.equal(image.readUInt16LE(pe + 24), 0x20b, "Expected PE32+");
assert.equal(image.readUInt16LE(pe + 24 + 68), 10, "Expected EFI application subsystem");
await copyFile(
  resolve(root, "tbt-retry.toml.example"),
  resolve(root, "dist/tbt-retry.toml"),
);
console.log("Built dist/EFI/BOOT/BOOTX64.EFI and dist/tbt-retry.toml");
