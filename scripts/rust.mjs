import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { resolve, delimiter } from 'node:path';
const env = { ...process.env, CARGO_BUILD_JOBS: process.env.CARGO_BUILD_JOBS ?? '2' };
const local = resolve('.tools/cargo');
if (existsSync(resolve(local, 'bin/cargo'))) {
  env.CARGO_HOME = local; env.RUSTUP_HOME = resolve('.tools/rustup'); env.PATH = resolve(local, 'bin') + delimiter + env.PATH;
}
const args = process.argv.slice(2); const command = args.shift() ?? 'test';
const subargs = command === 'fmt' ? [command, '--manifest-path', 'src-tauri/Cargo.toml', ...args] : [command, '--manifest-path', 'src-tauri/Cargo.toml', ...args];
const child = spawn('cargo', subargs, { stdio: 'inherit', env });
child.on('exit', code => process.exit(code ?? 1));
child.on('error', error => { console.error(error.message); process.exit(1); });
