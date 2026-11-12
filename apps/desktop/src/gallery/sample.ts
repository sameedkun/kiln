// Sample data for the design gallery's screens. In the app, all of this comes from `kilnd`'s
// API (P4-01): these shapes are a first draft of what its read methods return.
import type { StatusState } from "../components/Status";

export interface Site {
  name: string;
  secure: boolean;
  /** Detected by Kiln's drivers: shown as words, no logos. */
  framework?: string;
  php?: string;
  node?: string;
  folder?: string;
  proxy?: string;
  note?: string;
  services: string[];
}

export const siteGroups: { title: string; meta: string; sites: Site[] }[] = [
  {
    title: "Parked in D:\\Code",
    meta: "every folder inside is a site",
    sites: [
      { name: "shop.test", secure: true, framework: "Laravel 12", php: "8.4", node: "22", folder: "D:\\Code\\shop", services: ["MySQL", "Redis", "Mailpit", "Reverb"] },
      { name: "blog.test", secure: true, framework: "Laravel 11", php: "8.3", node: "22", folder: "D:\\Code\\blog", services: ["PostgreSQL"] },
      { name: "docs.test", secure: false, framework: "Static", php: "8.4", folder: "D:\\Code\\docs", services: [] },
      { name: "press.test", secure: true, framework: "WordPress 6", php: "8.3", folder: "D:\\Code\\press", services: ["MySQL"] },
    ],
  },
  {
    title: "Linked",
    meta: "folders added one by one",
    sites: [{ name: "billing.test", secure: true, framework: "Symfony 7", php: "8.4", node: "24", folder: "E:\\Work\\billing", services: ["MySQL", "Meilisearch"] }],
  },
  {
    title: "Proxies and tools",
    meta: "served by Kiln, run by something else",
    sites: [
      { name: "api.test", secure: false, proxy: "127.0.0.1:5173", note: "Vite dev server", services: [] },
      { name: "mailpit.test", secure: true, note: "Mailpit's inbox", services: ["Mailpit"] },
      { name: "pma.test", secure: true, note: "phpMyAdmin", services: ["MySQL", "MariaDB"] },
    ],
  },
];

export interface Service {
  id: string;
  name: string;
  version: string;
  category: "databases" | "cache" | "search" | "mail" | "storage" | "realtime";
  port: number;
  state: StatusState;
  usedBy: string[];
  /** A web page of its own, opened in the browser. */
  web?: string;
  /** A command-line client `kiln db` opens. */
  client?: boolean;
}

export const services: Service[] = [
  { id: "mysql", name: "MySQL", version: "8.4.6", category: "databases", port: 3306, state: "running", usedBy: ["shop", "press", "billing"], client: true },
  { id: "postgres", name: "PostgreSQL", version: "17.6", category: "databases", port: 5432, state: "running", usedBy: ["blog"], client: true },
  { id: "mariadb", name: "MariaDB", version: "11.8.3", category: "databases", port: 3307, state: "stopped", usedBy: [], client: true },
  { id: "redis", name: "Redis", version: "7.4.5", category: "cache", port: 6379, state: "attention", usedBy: ["shop"], client: true },
  { id: "meilisearch", name: "Meilisearch", version: "1.21.0", category: "search", port: 7700, state: "running", usedBy: ["billing"], web: "http://127.0.0.1:7700" },
  { id: "mailpit", name: "Mailpit", version: "1.27", category: "mail", port: 1025, state: "running", usedBy: ["shop"], web: "https://mailpit.test" },
  { id: "rustfs", name: "RustFS", version: "1.0.0-alpha.62", category: "storage", port: 9000, state: "running", usedBy: [], web: "http://127.0.0.1:9001" },
  { id: "reverb", name: "Reverb", version: "1.6.0", category: "realtime", port: 8080, state: "running", usedBy: ["shop"] },
];

export const categories = [
  { id: "all", label: "All" },
  { id: "databases", label: "Databases" },
  { id: "cache", label: "Cache" },
  { id: "search", label: "Search" },
  { id: "mail", label: "Mail" },
  { id: "storage", label: "Storage" },
  { id: "realtime", label: "Realtime" },
] as const;

export const catalog = [
  { name: "MySQL", versions: ["8.4", "8.0"], size: "296 MB", category: "Databases" },
  { name: "MariaDB", versions: ["11.8", "11.4"], size: "88 MB", category: "Databases" },
  { name: "PostgreSQL", versions: ["17", "16"], size: "312 MB", category: "Databases" },
  { name: "MongoDB", versions: ["9.0", "8.0"], size: "715 MB", category: "Databases" },
  { name: "Redis", versions: ["7.4"], size: "6 MB", category: "Cache" },
  { name: "Meilisearch", versions: ["1.21"], size: "120 MB", category: "Search" },
  { name: "Mailpit", versions: ["1.27"], size: "17 MB", category: "Mail" },
  { name: "RustFS", versions: ["1.0 alpha"], size: "61 MB", category: "Storage" },
];

export const phpVersions = [
  { version: "8.5.0", line: "8.5", sites: [], extensions: 14, isDefault: false },
  { version: "8.4.13", line: "8.4", sites: ["shop", "docs", "billing"], extensions: 16, isDefault: true, update: "8.4.14" },
  { version: "8.3.27", line: "8.3", sites: ["blog", "press"], extensions: 15, isDefault: false },
];

export const extensions = [
  { name: "bcmath", on: true },
  { name: "curl", on: true },
  { name: "gd", on: true },
  { name: "intl", on: true },
  { name: "mbstring", on: true },
  { name: "opcache", on: true },
  { name: "pdo_pgsql", on: true },
  { name: "redis", on: true },
  { name: "sodium", on: true },
  { name: "xdebug", on: false },
  { name: "imagick", on: false },
  { name: "soap", on: false },
];

export const nodeVersions = [
  { version: "24.11.0", line: "24", sites: ["billing"], isDefault: false },
  { version: "22.21.1", line: "22", sites: ["shop", "blog"], isDefault: true },
];

export const workers = [
  { id: "queue", name: "Queue worker", command: "php artisan queue:work", state: "running" as StatusState, detail: "2 processes" },
  { id: "schedule", name: "Scheduler", command: "php artisan schedule:work", state: "running" as StatusState, detail: "next run 14:15" },
  { id: "vite", name: "Vite", command: "npm run dev", state: "stopped" as StatusState, detail: "" },
  { id: "reverb", name: "Reverb", command: "php artisan reverb:start", state: "running" as StatusState, detail: ":8080" },
];

export const env = [
  { key: "APP_NAME", value: "Shop" },
  { key: "APP_URL", value: "https://shop.test", kiln: true },
  { key: "APP_KEY", value: "base64:9fKx…", secret: true },
  { key: "DB_CONNECTION", value: "mysql", kiln: true },
  { key: "DB_HOST", value: "127.0.0.1", kiln: true },
  { key: "DB_PORT", value: "3306", kiln: true },
  { key: "DB_DATABASE", value: "shop", kiln: true },
  { key: "REDIS_CLIENT", value: "predis", kiln: true },
  { key: "MAIL_PORT", value: "1025", kiln: true },
  { key: "STRIPE_SECRET", value: "sk_test_…", secret: true },
];

export const requests = {
  typical: "72 ms",
  slowest: "240 ms",
  count: "1,846",
  errors: "2.1%",
  errorCount: 38,
  buckets: [
    { label: "<25", value: 140 },
    { label: "<50", value: 410 },
    { label: "<100", value: 720 },
    { label: "<250", value: 380, tone: "slow" as const },
    { label: "<500", value: 120, tone: "slow" as const },
    { label: "<1s", value: 48, tone: "bad" as const },
    { label: "≥1s", value: 28, tone: "bad" as const },
  ],
  slow: [
    { time: "14:09:12", method: "GET", path: "/admin/orders?status=open", status: 200, ms: 1840 },
    { time: "14:07:55", method: "POST", path: "/checkout", status: 500, ms: 1210 },
    { time: "14:02:30", method: "GET", path: "/products/search?q=mug", status: 200, ms: 960 },
  ],
};

export const logSources = [
  { group: "Web server", items: [{ id: "caddy", label: "Caddy" }] },
  { group: "PHP errors", items: [{ id: "php-shop", label: "shop.test" }, { id: "php-blog", label: "blog.test" }] },
  { group: "Services", items: [{ id: "redis", label: "Redis", attention: true }, { id: "mysql", label: "MySQL" }, { id: "postgres", label: "PostgreSQL" }] },
  { group: "Kiln", items: [{ id: "kilnd", label: "kilnd" }] },
];

export const redisLog = [
  { time: "14:01:58", level: "info" as const, text: "DB saved on disk" },
  { time: "14:02:03", level: "warn" as const, text: "Memory usage 254.1M of maxmemory 256M (policy noeviction)" },
  { time: "14:02:04", level: "error" as const, text: "OOM command not allowed when used memory > 'maxmemory'" },
  { time: "14:02:04", level: "error" as const, text: "Guru Meditation: out of memory, exiting" },
  { time: "14:02:05", level: "info" as const, text: "kiln: Redis exited with code 1; not restarted (3 crashes in 10 minutes)" },
];

export const laravelLog = [
  { time: "14:07:55", level: "error" as const, text: "production.ERROR: Connection refused [tcp://127.0.0.1:6379] (Predis\\Connection\\ConnectionException)" },
  { time: "14:07:55", level: "info" as const, text: "  at vendor/predis/predis/src/Connection/AbstractConnection.php:144" },
  { time: "14:09:12", level: "warn" as const, text: "Slow query (812 ms): select * from `orders` where `status` = ? order by `created_at` desc" },
];

export const history = [
  {
    day: "Today",
    items: [
      { time: "14:10", what: "Linked api.test as a proxy to 127.0.0.1:5173", detail: "Caddy's config; the site list" },
      { time: "11:32", what: "Wired shop's .env to Redis", detail: "5 keys in D:\\Code\\shop\\.env" },
      { time: "09:05", what: "Turned on xdebug for PHP 8.4", detail: "php.ini for PHP 8.4", undone: true },
    ],
  },
  {
    day: "Yesterday",
    items: [
      { time: "17:48", what: "Installed PHP 8.4.13", detail: "a new folder under Kiln's runtimes" },
      { time: "16:20", what: "Added MySQL 8.4 as mysql", detail: "its data folder; port 3306" },
    ],
  },
  {
    day: "10 November",
    items: [{ time: "10:02", what: "Trusted Kiln's certificate authority", detail: "Windows' certificate store, current user" }],
  },
];

export const health = [
  { state: "running" as StatusState, text: "Kiln's certificate authority is trusted", detail: "HTTPS sites open without warnings." },
  { state: "running" as StatusState, text: "*.test names resolve", detail: "Through Kiln's block in the hosts file." },
  { state: "running" as StatusState, text: "Laravel Herd runs here too", detail: "Kiln uses ports 8080 and 8443; Herd keeps 80 and 443." },
  { state: "attention" as StatusState, text: "PHP 8.4.14 is out", detail: "shop.test, docs.test and billing.test use 8.4.13." },
];

export const usage = {
  memory: { value: 612, max: 32768, display: "612 MB", detail: "of 32 GB on this PC" },
  disk: { value: 3.1, max: 120, display: "3.1 GB", detail: "480 MB of it is downloads Kiln can clear" },
  top: [
    { name: "MySQL", memory: "410 MB" },
    { name: "PostgreSQL", memory: "96 MB" },
    { name: "PHP (8 workers)", memory: "64 MB" },
    { name: "Caddy", memory: "38 MB" },
  ],
};
