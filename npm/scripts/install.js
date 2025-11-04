#!/usr/bin/env node

const https = require('https');
const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');
const { promisify } = require('util');

const streamPipeline = promisify(require('stream').pipeline);

const VERSION = require('../package.json').version;
const REPO = 'kjanat/bun-docs-mcp-proxy';

function getPlatform() {
  const platform = process.platform;
  const arch = process.arch;

  // Map Node.js platform/arch to release archive names
  const platformMap = {
    'linux-x64': 'linux-x86_64',
    'linux-arm64': 'linux-aarch64',
    'darwin-x64': 'macos-x86_64',
    'darwin-arm64': 'macos-aarch64',
    'win32-x64': 'windows-x86_64',
    'win32-arm64': 'windows-aarch64',
  };

  const key = `${platform}-${arch}`;
  const target = platformMap[key];

  if (!target) {
    throw new Error(
      `Unsupported platform: ${platform} ${arch}\n` +
      `Please install from source: cargo install bun-docs-mcp-proxy`
    );
  }

  return { target, isWindows: platform === 'win32' };
}

async function download(url, destination) {
  return new Promise((resolve, reject) => {
    https.get(url, (response) => {
      if (response.statusCode === 302 || response.statusCode === 301) {
        // Follow redirect
        download(response.headers.location, destination).then(resolve).catch(reject);
        return;
      }

      if (response.statusCode !== 200) {
        reject(new Error(`Download failed: ${response.statusCode} ${response.statusMessage}`));
        return;
      }

      const fileStream = fs.createWriteStream(destination);
      response.pipe(fileStream);

      fileStream.on('finish', () => {
        fileStream.close();
        resolve();
      });

      fileStream.on('error', (err) => {
        fs.unlink(destination, () => {});
        reject(err);
      });
    }).on('error', reject);
  });
}

function extractTarGz(archivePath, outputDir, binaryName) {
  // Use native tar command (available on Unix and modern Windows)
  try {
    execSync(`tar -xzf "${archivePath}" -C "${outputDir}"`, { stdio: 'pipe' });

    // Move binary from extracted directory to bin directory
    const extractedBinary = path.join(outputDir, binaryName);
    if (!fs.existsSync(extractedBinary)) {
      // Binary might be in a subdirectory
      const files = fs.readdirSync(outputDir);
      for (const file of files) {
        const fullPath = path.join(outputDir, file, binaryName);
        if (fs.existsSync(fullPath)) {
          fs.renameSync(fullPath, extractedBinary);
          // Clean up directory
          fs.rmSync(path.join(outputDir, file), { recursive: true, force: true });
          break;
        }
      }
    }
  } catch (error) {
    throw new Error(`Failed to extract tar.gz archive: ${error.message}`);
  }
}

function extractZip(archivePath, outputDir, binaryName) {
  // Use PowerShell on Windows (always available)
  try {
    execSync(`powershell -Command "Expand-Archive -Path '${archivePath}' -DestinationPath '${outputDir}' -Force"`, { stdio: 'pipe' });

    // Move binary from extracted directory to bin directory if needed
    const extractedBinary = path.join(outputDir, binaryName);
    if (!fs.existsSync(extractedBinary)) {
      // Binary might be in a subdirectory
      const files = fs.readdirSync(outputDir);
      for (const file of files) {
        const fullPath = path.join(outputDir, file, binaryName);
        if (fs.existsSync(fullPath)) {
          fs.renameSync(fullPath, extractedBinary);
          // Clean up directory
          fs.rmSync(path.join(outputDir, file), { recursive: true, force: true });
          break;
        }
      }
    }
  } catch (error) {
    throw new Error(`Failed to extract zip archive: ${error.message}`);
  }
}

async function install() {
  try {
    const { target, isWindows } = getPlatform();
    const binaryName = isWindows ? 'bun-docs-mcp-proxy.exe' : 'bun-docs-mcp-proxy';
    const archiveExt = isWindows ? 'zip' : 'tar.gz';
    const archiveName = `bun-docs-mcp-proxy-${target}.${archiveExt}`;

    const downloadUrl = `https://github.com/${REPO}/releases/download/v${VERSION}/${archiveName}`;
    const binDir = path.join(__dirname, '..', 'bin');
    const archivePath = path.join(binDir, archiveName);
    const binaryPath = path.join(binDir, binaryName);

    // Check if binary already exists
    if (fs.existsSync(binaryPath)) {
      console.log('Binary already installed');
      return;
    }

    console.log(`Downloading bun-docs-mcp-proxy v${VERSION} for ${target}...`);
    console.log(`URL: ${downloadUrl}`);

    await download(downloadUrl, archivePath);
    console.log('Download complete, extracting...');

    if (isWindows) {
      extractZip(archivePath, binDir, binaryName);
    } else {
      extractTarGz(archivePath, binDir, binaryName);
    }

    // Clean up archive
    fs.unlinkSync(archivePath);

    // Make binary executable (Unix-like systems)
    if (!isWindows) {
      fs.chmodSync(binaryPath, 0o755);
    }

    console.log('Installation complete!');
    console.log(`Binary installed at: ${binaryPath}`);
  } catch (error) {
    console.error('Installation failed:', error.message);
    console.error('\nYou can install from source using:');
    console.error('  cargo install bun-docs-mcp-proxy');
    process.exit(1);
  }
}

install();
