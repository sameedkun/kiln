// The app makes no network calls of its own (ADR 0051 §5): fonts and icons are bundled.
// Fails if the built page or styles point anywhere off the machine, or if the scripts name
// a font or script CDN.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const dist = new URL("../dist/", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const files = [];
(function walk(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path);
    else files.push(path);
  }
})(dist);

const problems = [];
for (const file of files) {
  const text = /\.(html|css|js)$/.test(file) ? readFileSync(file, "utf8") : "";
  if (/\.(html|css)$/.test(file)) {
    for (const [url] of text.matchAll(/(?:https?:)?\/\/[a-z0-9.-]+\.[a-z]{2,}[^\s"')]*/gi)) {
      // XML namespaces in inline SVG aren't fetched.
      if (url.startsWith("http://www.w3.org/")) continue;
      problems.push(`${file}: ${url}`);
    }
  }
  if (file.endsWith(".js")) {
    for (const host of ["fonts.googleapis", "fonts.gstatic", "cdn.jsdelivr", "unpkg.com", "cdnjs."]) {
      if (text.includes(host)) problems.push(`${file}: names ${host}`);
    }
  }
}

if (problems.length) {
  console.error(`The build points off the machine:\n${problems.join("\n")}`);
  process.exit(1);
}
console.log(`offline: ${files.length} files checked, nothing points off the machine`);
