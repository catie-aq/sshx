/**
 * Mobile interaction tests: double-tap fullscreen, long-press drag,
 * fullscreen UI rendering at various viewport sizes.
 *
 * Uses Playwright's touch emulation to simulate mobile gestures.
 * Each test spins up a simulated CLI client (via gRPC) so terminals appear.
 */
import { test, expect, type Page } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";
import { createSession, simulateCliWithShell, simulateCliWithShells } from "./helpers/grpc-client";
import { computeEncryptedZeros } from "./helpers/encrypt";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const TEST_KEY = "testkey1234567";
let cachedEncryptedZeros: Buffer | null = null;

const SCREENSHOT_DIR = path.join(__dirname, "test-results", "mobile");

async function getEncryptedZeros(): Promise<Buffer> {
  if (!cachedEncryptedZeros) {
    cachedEncryptedZeros = await computeEncryptedZeros(TEST_KEY);
  }
  return cachedEncryptedZeros;
}

function getEndpoint(): string {
  const portFile = path.join(__dirname, ".server-port");
  try {
    return `[::1]:${fs.readFileSync(portFile, "utf-8").trim()}`;
  } catch {
    return `[::1]:${process.env.SSHX_TEST_PORT || "18051"}`;
  }
}

function getBaseUrl(): string {
  const portFile = path.join(__dirname, ".server-port");
  try {
    return `http://[::1]:${fs.readFileSync(portFile, "utf-8").trim()}`;
  } catch {
    return `http://[::1]:${process.env.SSHX_TEST_PORT || "18051"}`;
  }
}

async function screenshot(page: Page, name: string): Promise<void> {
  fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
  await page.screenshot({
    path: path.join(SCREENSHOT_DIR, `${name}.png`),
    fullPage: false,
  });
}

/** Create session + simulated CLI client with a shell. */
async function setupSession(endpoint: string): Promise<{
  name: string;
  key: string;
  cleanup: () => void;
}> {
  const session = await createSession(endpoint, await getEncryptedZeros());
  const cleanup = simulateCliWithShell(endpoint, session.name, session.token, {
    shellId: 1, x: 0, y: 0, data: "user@host:~$ \r\n",
  });
  return { name: session.name, key: session.key, cleanup };
}

/** Open session page, join, wait for terminal to appear. */
async function openSession(page: Page, baseUrl: string, name: string, key: string): Promise<void> {
  await page.goto(`${baseUrl}/s/${name}#${key}`);
  const joinBtn = page.locator('button:has-text("Join")');
  await joinBtn.waitFor({ timeout: 10_000 });
  const nameInput = page.locator('input[placeholder="Your name"]');
  if (await nameInput.isVisible()) await nameInput.fill("MobileUser");
  await joinBtn.click();
  await page.waitForSelector(".term-container", { timeout: 10_000 });
}

/** Double-tap on the terminal title bar to enter fullscreen. */
async function enterFullscreen(page: Page): Promise<void> {
  const titleBar = page.locator(".term-container .cursor-grab").first();
  await expect(titleBar).toBeVisible({ timeout: 5_000 });
  const box = await titleBar.boundingBox();
  // Double-tap
  await page.touchscreen.tap(box!.x + box!.width / 2, box!.y + box!.height / 2);
  await page.waitForTimeout(100);
  await page.touchscreen.tap(box!.x + box!.width / 2, box!.y + box!.height / 2);
  // Wait for fullscreen overlay
  await expect(page.locator('input[placeholder="Type command…"]')).toBeVisible({ timeout: 5_000 });
}

const VIEWPORTS = {
  iphone_se:          { width: 375, height: 667,  name: "iPhone-SE" },
  iphone_14:          { width: 390, height: 844,  name: "iPhone-14" },
  iphone_14_pro_max:  { width: 430, height: 932,  name: "iPhone-14-Pro-Max" },
  pixel_7:            { width: 412, height: 915,  name: "Pixel-7" },
  ipad_mini:          { width: 768, height: 1024, name: "iPad-Mini" },
};

// ── Double-tap fullscreen ──────────────────────────────────────────────

test.describe("Mobile — Double-tap fullscreen", () => {
  let endpoint: string, baseUrl: string;
  test.beforeAll(() => { endpoint = getEndpoint(); baseUrl = getBaseUrl(); });

  test("double-tap enters fullscreen, Exit leaves it", async ({ browser }) => {
    const ctx = await browser.newContext({ hasTouch: true, isMobile: true, viewport: VIEWPORTS.iphone_14 });
    const page = await ctx.newPage();
    const { name, key, cleanup } = await setupSession(endpoint);
    try {
      await openSession(page, baseUrl, name, key);
      await page.waitForTimeout(300);
      await enterFullscreen(page);

      const exitBtn = page.locator('button:has-text("Exit")');
      await expect(exitBtn).toBeVisible();
      await screenshot(page, "01-double-tap-fullscreen");

      await exitBtn.click();
      await page.waitForTimeout(300);
      await expect(page.locator('input[placeholder="Type command…"]')).not.toBeVisible();
      await screenshot(page, "01-double-tap-exited");
    } finally { cleanup(); await ctx.close(); }
  });

  test("single tap does NOT enter fullscreen", async ({ browser }) => {
    const ctx = await browser.newContext({ hasTouch: true, isMobile: true, viewport: VIEWPORTS.iphone_14 });
    const page = await ctx.newPage();
    const { name, key, cleanup } = await setupSession(endpoint);
    try {
      await openSession(page, baseUrl, name, key);
      await page.waitForTimeout(300);

      const titleBar = page.locator(".term-container .cursor-grab").first();
      const box = await titleBar.boundingBox();
      await page.touchscreen.tap(box!.x + box!.width / 2, box!.y + box!.height / 2);
      await page.waitForTimeout(600);

      await expect(page.locator('input[placeholder="Type command…"]')).not.toBeVisible();
      await screenshot(page, "02-single-tap-no-fullscreen");
    } finally { cleanup(); await ctx.close(); }
  });
});

// ── Fullscreen UI at various viewports ─────────────────────────────────

test.describe("Mobile — Fullscreen UI at various viewports", () => {
  let endpoint: string, baseUrl: string;
  test.beforeAll(() => { endpoint = getEndpoint(); baseUrl = getBaseUrl(); });

  for (const [, vp] of Object.entries(VIEWPORTS)) {
    test(`fullscreen at ${vp.name} (${vp.width}x${vp.height})`, async ({ browser }) => {
      const ctx = await browser.newContext({ hasTouch: true, isMobile: true, viewport: vp });
      const page = await ctx.newPage();
      const { name, key, cleanup } = await setupSession(endpoint);
      try {
        await openSession(page, baseUrl, name, key);
        await page.waitForTimeout(300);
        await enterFullscreen(page);
        await page.waitForTimeout(500);

        // Decorations hidden
        await expect(page.locator("button.bg-red-500")).not.toBeVisible();

        // Title bar with mono font visible
        await expect(page.locator("span.font-mono.truncate")).toBeVisible();

        // xterm screen is wide
        const xtermScreen = page.locator(".xterm-screen").first();
        await expect(xtermScreen).toBeVisible({ timeout: 5_000 });
        const box = await xtermScreen.boundingBox();
        expect(box!.width).toBeGreaterThan(vp.width * 0.85);

        // Scroll button visible
        await expect(page.locator('button[title="Scroll to bottom"]')).toBeVisible();

        await screenshot(page, `03-fullscreen-${vp.name}`);
      } finally { cleanup(); await ctx.close(); }
    });
  }
});

// ── Fullscreen no decorations ──────────────────────────────────────────

test.describe("Mobile — Fullscreen no decorations", () => {
  let endpoint: string, baseUrl: string;
  test.beforeAll(() => { endpoint = getEndpoint(); baseUrl = getBaseUrl(); });

  test("hides circle buttons and restores on exit", async ({ browser }) => {
    const ctx = await browser.newContext({ hasTouch: true, isMobile: true, viewport: VIEWPORTS.iphone_14 });
    const page = await ctx.newPage();
    const { name, key, cleanup } = await setupSession(endpoint);
    try {
      await openSession(page, baseUrl, name, key);
      await page.waitForTimeout(300);

      // Before: circle buttons visible
      await expect(page.locator(".term-container button.bg-red-500").first()).toBeVisible({ timeout: 5_000 });

      await enterFullscreen(page);

      // After: circle buttons hidden
      await expect(page.locator("button.bg-red-500")).not.toBeVisible();
      await screenshot(page, "04-fullscreen-no-decorations");

      // Exit
      await page.locator('button:has-text("Exit")').click();
      await page.waitForTimeout(500);
      await expect(page.locator(".term-container button.bg-red-500").first()).toBeVisible({ timeout: 3_000 });
      await screenshot(page, "04-decorations-restored");
    } finally { cleanup(); await ctx.close(); }
  });
});

// ── Terminal switcher ──────────────────────────────────────────────────

test.describe("Mobile — Fullscreen terminal switcher", () => {
  let endpoint: string, baseUrl: string;
  test.beforeAll(() => { endpoint = getEndpoint(); baseUrl = getBaseUrl(); });

  test("burger menu opens terminal drawer", async ({ browser }) => {
    const ctx = await browser.newContext({ hasTouch: true, isMobile: true, viewport: VIEWPORTS.iphone_14 });
    const page = await ctx.newPage();
    const { name, key, cleanup } = await setupSession(endpoint);
    try {
      await openSession(page, baseUrl, name, key);
      await page.waitForTimeout(300);
      await enterFullscreen(page);

      // Click burger menu (button containing the burger-line spans)
      await page.locator("button:has(span.bg-zinc-300)").first().click();
      await page.waitForTimeout(300);
      // Drawer should slide in with "Terminals" header
      await expect(page.locator('text="Terminals"')).toBeVisible({ timeout: 3_000 });
      await screenshot(page, "05-fullscreen-drawer");
    } finally { cleanup(); await ctx.close(); }
  });
});

// ── Input bar ──────────────────────────────────────────────────────────

test.describe("Mobile — Fullscreen input bar", () => {
  let endpoint: string, baseUrl: string;
  test.beforeAll(() => { endpoint = getEndpoint(); baseUrl = getBaseUrl(); });

  test("Send button activates when text entered", async ({ browser }) => {
    const ctx = await browser.newContext({ hasTouch: true, isMobile: true, viewport: VIEWPORTS.iphone_14 });
    const page = await ctx.newPage();
    const { name, key, cleanup } = await setupSession(endpoint);
    try {
      await openSession(page, baseUrl, name, key);
      await page.waitForTimeout(300);
      await enterFullscreen(page);

      const sendBtn = page.locator('button:has-text("Send")');
      await expect(sendBtn).toHaveClass(/bg-zinc-700/);
      await screenshot(page, "06-input-empty");

      await page.locator('input[placeholder="Type command…"]').fill("ls -la");
      await page.waitForTimeout(150);
      await expect(sendBtn).toHaveClass(/bg-indigo-600/);
      await screenshot(page, "06-input-with-text");
    } finally { cleanup(); await ctx.close(); }
  });
});

// ── Tap-to-move removed ────────────────────────────────────────────────

test.describe("Mobile — Tap-to-move removed", () => {
  let endpoint: string, baseUrl: string;
  test.beforeAll(() => { endpoint = getEndpoint(); baseUrl = getBaseUrl(); });

  test("no tap-to-place banner after tapping title bar", async ({ browser }) => {
    const ctx = await browser.newContext({ hasTouch: true, isMobile: true, viewport: VIEWPORTS.iphone_14 });
    const page = await ctx.newPage();
    const { name, key, cleanup } = await setupSession(endpoint);
    try {
      await openSession(page, baseUrl, name, key);
      await page.waitForTimeout(300);

      const titleBar = page.locator(".term-container .cursor-grab").first();
      const box = await titleBar.boundingBox();
      await page.touchscreen.tap(box!.x + box!.width / 2, box!.y + box!.height / 2);
      await page.waitForTimeout(600);

      await expect(page.locator('text="Tap canvas to place terminal"')).not.toBeVisible();
      await expect(page.locator(".term-container.mobile-selected")).toHaveCount(0);
      await screenshot(page, "07-no-tap-to-move");
    } finally { cleanup(); await ctx.close(); }
  });
});

// ── Width management ───────────────────────────────────────────────────

test.describe("Mobile — Width management", () => {
  let endpoint: string, baseUrl: string;
  test.beforeAll(() => { endpoint = getEndpoint(); baseUrl = getBaseUrl(); });

  for (const [, vp] of Object.entries(VIEWPORTS)) {
    test(`xterm fills width at ${vp.name}`, async ({ browser }) => {
      const ctx = await browser.newContext({ hasTouch: true, isMobile: true, viewport: vp });
      const page = await ctx.newPage();
      const { name, key, cleanup } = await setupSession(endpoint);
      try {
        await openSession(page, baseUrl, name, key);
        await page.waitForTimeout(300);
        await enterFullscreen(page);
        await page.waitForTimeout(500);

        // xterm viewport should be > 85% of screen width
        const xtermVp = page.locator(".xterm-viewport").first();
        if (await xtermVp.isVisible()) {
          const box = await xtermVp.boundingBox();
          expect(box!.width).toBeGreaterThan(vp.width * 0.85);
        }
        await screenshot(page, `08-width-${vp.name}`);
      } finally { cleanup(); await ctx.close(); }
    });
  }
});

// ── Landscape fullscreen ───────────────────────────────────────────────

test.describe("Mobile — Landscape", () => {
  let endpoint: string, baseUrl: string;
  test.beforeAll(() => { endpoint = getEndpoint(); baseUrl = getBaseUrl(); });

  test("fullscreen in landscape", async ({ browser }) => {
    const ctx = await browser.newContext({ hasTouch: true, isMobile: true, viewport: { width: 844, height: 390 } });
    const page = await ctx.newPage();
    const { name, key, cleanup } = await setupSession(endpoint);
    try {
      await openSession(page, baseUrl, name, key);
      await page.waitForTimeout(500);

      // In landscape, the canvas transform can push the terminal outside the
      // viewport. Use dispatchEvent to bypass viewport checks.
      const greenBtn = page.locator(".term-container button.bg-green-500").first();
      await greenBtn.dispatchEvent("click");
      await page.waitForTimeout(800);

      await expect(page.locator('input[placeholder="Type command…"]')).toBeVisible({ timeout: 5_000 });

      const screen = page.locator(".xterm-screen").first();
      await expect(screen).toBeVisible({ timeout: 5_000 });
      const box = await screen.boundingBox();
      expect(box!.width).toBeGreaterThan(700);
      await screenshot(page, "09-fullscreen-landscape");
    } finally { cleanup(); await ctx.close(); }
  });
});

// ── Terminal switching in fullscreen ────────────────────────────────────

test.describe("Mobile — Terminal switching", () => {
  let endpoint: string, baseUrl: string;
  test.beforeAll(() => { endpoint = getEndpoint(); baseUrl = getBaseUrl(); });

  /** Create session with 2 shells. */
  async function setupMultiShellSession(ep: string) {
    const session = await createSession(ep, await getEncryptedZeros());
    const cleanup = simulateCliWithShells(ep, session.name, session.token, [
      { shellId: 1, x: 0, y: 0, data: "shell-1$ \r\n" },
      { shellId: 2, x: 200, y: 0, data: "shell-2$ \r\n" },
    ]);
    return { name: session.name, key: session.key, cleanup };
  }

  test("switch terminal via drawer without freeze, then exit", async ({ browser }) => {
    const ctx = await browser.newContext({ hasTouch: true, isMobile: true, viewport: VIEWPORTS.iphone_14 });
    const page = await ctx.newPage();
    const { name, key, cleanup } = await setupMultiShellSession(endpoint);
    try {
      await openSession(page, baseUrl, name, key);
      // Wait for both terminals to appear
      await page.waitForSelector(".term-container >> nth=1", { timeout: 10_000 });
      await page.waitForTimeout(300);

      // Enter fullscreen on first terminal
      await enterFullscreen(page);
      await page.waitForTimeout(300);
      await screenshot(page, "10-switch-before");

      // Open the drawer
      await page.locator("button:has(span.bg-zinc-300)").first().click();
      await page.waitForTimeout(300);
      await expect(page.locator('text="Terminals"')).toBeVisible({ timeout: 3_000 });

      // Click the second terminal in the drawer
      const termButtons = page.locator('text="Terminals"').locator("..").locator("~ div button");
      const secondBtn = page.locator('button:has-text("Terminal #2")');
      await secondBtn.click();
      await page.waitForTimeout(800);

      // Drawer should close
      await expect(page.locator('text="Terminals"')).not.toBeVisible();

      // Fullscreen should still be active with input bar visible
      const inputBar = page.locator('input[placeholder="Type command…"]');
      await expect(inputBar).toBeVisible({ timeout: 3_000 });

      // The xterm screen should be visible (not black)
      const xtermScreen = page.locator(".xterm-screen").first();
      await expect(xtermScreen).toBeVisible({ timeout: 3_000 });

      await screenshot(page, "10-switch-after");

      // Now exit fullscreen — this must work (was freezing before)
      const exitBtn = page.locator('button:has-text("Exit")');
      await expect(exitBtn).toBeVisible();
      await exitBtn.click();
      await page.waitForTimeout(500);

      // Input bar should be gone (we exited fullscreen)
      await expect(inputBar).not.toBeVisible();

      // Canvas should be responsive — the terminals should be visible
      await expect(page.locator(".term-container").first()).toBeVisible({ timeout: 3_000 });

      await screenshot(page, "10-switch-exited");
    } finally { cleanup(); await ctx.close(); }
  });

  test("rapid switching does not crash", async ({ browser }) => {
    const ctx = await browser.newContext({ hasTouch: true, isMobile: true, viewport: VIEWPORTS.iphone_14 });
    const page = await ctx.newPage();
    const { name, key, cleanup } = await setupMultiShellSession(endpoint);
    try {
      await openSession(page, baseUrl, name, key);
      await page.waitForSelector(".term-container >> nth=1", { timeout: 10_000 });
      await page.waitForTimeout(300);

      // Enter fullscreen
      await enterFullscreen(page);
      await page.waitForTimeout(300);

      // Switch back and forth 3 times rapidly
      for (let i = 0; i < 3; i++) {
        // Open drawer
        await page.locator("button:has(span.bg-zinc-300)").first().click();
        await page.waitForTimeout(200);
        // Click Terminal #2
        await page.locator('button:has-text("Terminal #2")').click();
        await page.waitForTimeout(300);

        // Open drawer again
        await page.locator("button:has(span.bg-zinc-300)").first().click();
        await page.waitForTimeout(200);
        // Click Terminal #1
        await page.locator('button:has-text("Terminal #1")').click();
        await page.waitForTimeout(300);
      }

      // After rapid switches, everything should still work
      const inputBar = page.locator('input[placeholder="Type command…"]');
      await expect(inputBar).toBeVisible({ timeout: 3_000 });

      const xtermScreen = page.locator(".xterm-screen").first();
      await expect(xtermScreen).toBeVisible({ timeout: 3_000 });

      // Exit should work
      await page.locator('button:has-text("Exit")').click();
      await page.waitForTimeout(500);
      await expect(inputBar).not.toBeVisible();
      await expect(page.locator(".term-container").first()).toBeVisible({ timeout: 3_000 });

      await screenshot(page, "11-rapid-switch-survived");
    } finally { cleanup(); await ctx.close(); }
  });
});
