#!/usr/bin/env bun

const arches = ["aarch64", "x86_64"] as const;
const libcs = ["gnu", "musl"] as const;

type Arch = typeof arches[number];
type Libc = typeof libcs[number];
type Fam = "macos" | "linux" | "windows";

const mk = (os: string, name: string, target: string, label: string) => {
  const win = os[0] === "w";
  return {
    os,
    name,
    target,
    label,
    archive_ext: win ? "zip" : "tar.gz",
    bin_ext: win ? ".exe" : "",
    cross: os[0] === "u" && (target.includes("aarch64") || target.includes("musl")),
  };
};

const label = (fam: Fam, arch: Arch, libc?: Libc) =>
  fam === "macos"
    ? (arch === "x86_64" ? "macOS Intel" : "macOS Apple Silicon")
    : fam === "windows"
    ? `Windows ${arch === "aarch64" ? "ARM64" : "x86_64"}`
    : `Linux ${arch === "aarch64" ? "ARM64" : "x86_64"}${libc === "musl" ? " musl (static)" : ""}`;

const platforms = [
  mk("macos-15-intel", "macos-x86_64", "x86_64-apple-darwin", label("macos", "x86_64")),
  mk("macos-latest", "macos-aarch64", "aarch64-apple-darwin", label("macos", "aarch64")),

  ...arches.flatMap(a =>
    libcs.map(l =>
      mk("ubuntu-latest", `linux-${a}${l === "musl" ? "-musl" : ""}`, `${a}-unknown-linux-${l}`, label("linux", a, l))
    )
  ),

  ...arches.map(a => mk("windows-latest", `windows-${a}`, `${a}-pc-windows-msvc`, label("windows", a))),
];

console.log(JSON.stringify(platforms));
