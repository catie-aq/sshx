/**
 * Comprehensive screenshot tests for all main UI components.
 *
 * Each test creates a fresh session, sets up a specific component,
 * and captures a screenshot saved to tests/e2e/test-results/screenshots/.
 */
import { test, expect, type Page } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";
import { createSession } from "./helpers/grpc-client";
import { computeEncryptedZeros } from "./helpers/encrypt";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const TEST_KEY = "testkey1234567";
let cachedEncryptedZeros: Buffer | null = null;

const SCREENSHOT_DIR = path.join(__dirname, "test-results", "screenshots");

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

/** Open a session page, fill the name dialog, and wait for connection. */
async function openSession(
  page: Page,
  baseUrl: string,
  sessionName: string,
  key: string
): Promise<void> {
  await page.goto(`${baseUrl}/s/${sessionName}#${key}`);
  const joinBtn = page.locator('button:has-text("Join")');
  await joinBtn.waitFor({ timeout: 10_000 });
  const nameInput = page.locator("input[placeholder]");
  if (await nameInput.isVisible()) {
    await nameInput.fill("TestUser");
  }
  await joinBtn.click();
  await page.waitForSelector('button[title="Notes"]', { timeout: 10_000 });
}

/** Switch to creative mode and wait for write-access buttons to be enabled. */
async function switchToCreativeMode(page: Page): Promise<void> {
  await page.locator('button[title="Notes"]').click();
  // Wait for the sticky note button with its "connected + write" title to appear and be enabled.
  // This ensures the user has write access before interacting with creative tools.
  await page.waitForSelector('button[title="Add sticky note"]:not([disabled])', {
    timeout: 10_000,
  });
}

/** Take a screenshot and save to the screenshots directory. */
async function screenshot(page: Page, name: string): Promise<void> {
  fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
  await page.screenshot({
    path: path.join(SCREENSHOT_DIR, `${name}.png`),
    fullPage: false,
  });
}

/** Take a screenshot of a specific element. */
async function screenshotElement(
  page: Page,
  selector: string,
  name: string
): Promise<void> {
  fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
  const el = page.locator(selector).first();
  await el.waitFor({ timeout: 5_000 });
  await el.screenshot({
    path: path.join(SCREENSHOT_DIR, `${name}.png`),
  });
}

test.describe("Screenshot Tests", () => {
  let endpoint: string;
  let baseUrl: string;

  test.beforeAll(() => {
    endpoint = getEndpoint();
    baseUrl = getBaseUrl();
  });

  test("01 - Main UI in terminal mode", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);

    // Wait for UI to settle
    await page.waitForTimeout(500);

    await screenshot(page, "01-main-ui-terminal-mode");

    // Verify the toolbar is visible
    const toolbar = page.locator(".panel.inline-block");
    await expect(toolbar).toBeVisible();
  });

  test("02 - Main UI in creative mode", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await switchToCreativeMode(page);
    await page.waitForTimeout(300);

    await screenshot(page, "02-main-ui-creative-mode");
  });

  test("03 - Toolbar close-up", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await switchToCreativeMode(page);
    await page.waitForTimeout(300);

    await screenshotElement(page, ".panel.inline-block", "03-toolbar");
  });

  test("04 - Sticky note (yellow)", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await switchToCreativeMode(page);

    // Create a sticky note
    await page.locator('button[title*="sticky note"]').click();
    await page.waitForTimeout(500);

    // Wait for the note to appear
    const note = page.locator(".sticky-note").first();
    await expect(note).toBeVisible({ timeout: 5_000 });

    // Type some content into the note
    const noteBody = page.locator(".note-body").first();
    await noteBody.click();
    await page.waitForTimeout(200);
    await noteBody.evaluate((el) => {
      const tiptap = el.querySelector(".tiptap");
      if (tiptap) {
        tiptap.innerHTML = "<p><strong>Meeting Notes</strong></p><p>Discuss the new feature roadmap</p><ul><li>Timeline review</li><li>Resource allocation</li></ul>";
        tiptap.dispatchEvent(new Event("input", { bubbles: true }));
      }
    });
    await page.waitForTimeout(300);

    await screenshotElement(page, ".sticky-note", "04-sticky-note-yellow");
  });

  test("05 - Sticky notes (all colors)", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await switchToCreativeMode(page);

    // Create multiple notes by clicking the button several times
    for (let i = 0; i < 3; i++) {
      await page.locator('button[title*="sticky note"]').click();
      await page.waitForTimeout(400);
    }

    // Wait for notes
    const notes = page.locator(".sticky-note");
    await expect(notes.first()).toBeVisible({ timeout: 5_000 });

    // Change colors on the different notes by clicking color swatches
    const noteHeaders = page.locator(".sticky-note");
    const count = await noteHeaders.count();
    const colors = ["pink", "blue", "green"];
    for (let i = 0; i < Math.min(count, colors.length); i++) {
      const header = noteHeaders.nth(i);
      // Click a color swatch in each note's header
      const swatches = header.locator("button.rounded-full.border");
      const swatchCount = await swatches.count();
      if (swatchCount > i + 1) {
        await swatches.nth(i + 1).click();
        await page.waitForTimeout(200);
      }
    }

    await page.waitForTimeout(300);
    await screenshot(page, "05-sticky-notes-colors");
  });

  test("06 - Text block with toolbar", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await switchToCreativeMode(page);

    // Activate text tool
    await page.locator('button[title*="text block"]').click();

    // Place a text block
    const canvas = page.locator(".absolute.inset-0.overflow-hidden.touch-none");
    const box = await canvas.boundingBox();
    await canvas.click({ position: { x: box!.width / 2, y: box!.height / 2 } });
    await page.waitForTimeout(500);

    const content = page.locator(".text-content").first();
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Enter editing and type
    await content.click();
    await content.click();
    await page.waitForTimeout(200);
    await content.evaluate((el) => {
      el.innerHTML = "<p><strong>Hello World</strong> — rich text on an infinite canvas</p>";
      el.dispatchEvent(new Event("input", { bubbles: true }));
    });

    await page.waitForTimeout(300);

    // Screenshot the text block with its toolbar visible
    await screenshotElement(page, ".text-block-wrapper", "06-text-block-with-toolbar");

    // Also full page view
    await screenshot(page, "06-text-block-fullpage");
  });

  test("07 - Text block font sizes", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await switchToCreativeMode(page);

    // Create text block
    await page.locator('button[title*="text block"]').click();
    const canvas = page.locator(".absolute.inset-0.overflow-hidden.touch-none");
    const box = await canvas.boundingBox();
    await canvas.click({ position: { x: box!.width / 2, y: box!.height / 2 } });
    await page.waitForTimeout(500);

    const content = page.locator(".text-content").first();
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Enter editing, type content
    await content.click();
    await content.click();
    await content.evaluate((el) => {
      el.innerHTML = "<p>Large heading text</p>";
      el.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await page.waitForTimeout(200);

    // Change to XL size via toolbar
    const wrapper = page.locator(".text-block-wrapper").first();
    const fontSelect = wrapper.locator(".toolbar-select");
    await fontSelect.selectOption("xl");
    await page.waitForTimeout(300);

    await screenshotElement(page, ".text-block-wrapper", "07-text-block-xl-font");
  });

  test("08 - Drawing toolbar (pencil active)", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await switchToCreativeMode(page);

    // Activate pencil tool
    await page.locator('button[title="Pencil"]').click();
    await page.waitForTimeout(300);

    // The drawing toolbar should appear
    const drawingToolbar = page.locator(".drawing-toolbar");
    await expect(drawingToolbar).toBeVisible({ timeout: 3_000 });

    await screenshotElement(page, ".drawing-toolbar", "08-drawing-toolbar-pencil");
    await screenshot(page, "08-drawing-mode-fullpage");
  });

  test("09 - Drawing toolbar (highlighter active)", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await switchToCreativeMode(page);

    // Activate highlighter
    await page.locator('button[title="Highlighter"]').click();
    await page.waitForTimeout(300);

    const drawingToolbar = page.locator(".drawing-toolbar");
    await expect(drawingToolbar).toBeVisible({ timeout: 3_000 });

    await screenshotElement(page, ".drawing-toolbar", "09-drawing-toolbar-highlighter");
  });

  test("10 - Drawing strokes on canvas", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await switchToCreativeMode(page);

    // Activate pencil
    await page.locator('button[title="Pencil"]').click();
    await page.waitForTimeout(300);

    // Draw a stroke on the canvas
    const canvas = page.locator(".drawing-layer");
    const box = await canvas.boundingBox();
    if (box) {
      const startX = box.width * 0.3;
      const startY = box.height * 0.5;
      await page.mouse.move(startX, startY);
      await page.mouse.down();
      // Draw a wavy line
      for (let i = 0; i < 20; i++) {
        await page.mouse.move(
          startX + i * 15,
          startY + Math.sin(i * 0.5) * 30,
          { steps: 2 }
        );
      }
      await page.mouse.up();
    }
    await page.waitForTimeout(300);

    // Now switch to highlighter and draw another stroke
    // Use the DrawingToolbar's highlighter button (the toolbar one is disabled while pencil is active)
    await page.locator('.tool-btn[title="Highlighter"]').click();
    await page.waitForTimeout(200);
    if (box) {
      const startX = box.width * 0.25;
      const startY = box.height * 0.4;
      await page.mouse.move(startX, startY);
      await page.mouse.down();
      for (let i = 0; i < 15; i++) {
        await page.mouse.move(startX + i * 20, startY, { steps: 2 });
      }
      await page.mouse.up();
    }
    await page.waitForTimeout(300);

    // Deactivate tool to take screenshot
    await page.keyboard.press("Escape");
    await page.waitForTimeout(200);

    await screenshot(page, "10-drawing-strokes");
  });

  test("11 - Slideshow mode with timeline", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await switchToCreativeMode(page);

    // Toggle slideshow mode
    await page.locator('button[title*="Slideshow"]').click();
    await page.waitForTimeout(300);

    // Timeline panel should appear on the right
    const timeline = page.locator(".timeline-panel");
    await expect(timeline).toBeVisible({ timeout: 3_000 });

    await screenshotElement(page, ".timeline-panel", "11-timeline-empty");

    // Create a slide
    await page.locator('button[title="Add slide"]').click();
    await page.waitForTimeout(500);

    // Create another slide
    await page.locator('button[title="Add slide"]').click();
    await page.waitForTimeout(500);

    await screenshot(page, "11-slideshow-mode");
    await screenshotElement(page, ".timeline-panel", "11-timeline-with-slides");
  });

  test("12 - Slide region on canvas", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await switchToCreativeMode(page);

    // Toggle slideshow mode and create a slide
    await page.locator('button[title*="Slideshow"]').click();
    await page.waitForTimeout(300);
    await page.locator('button[title="Add slide"]').click();
    await page.waitForTimeout(500);

    // Check that the slide region appears
    const slideRegion = page.locator(".slide-region");
    await expect(slideRegion.first()).toBeVisible({ timeout: 5_000 });

    await screenshotElement(page, ".slide-region", "12-slide-region");
  });

  test("13 - Chat panel", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);

    // Open chat
    await page.locator('button[title="Chat"]').click();
    await page.waitForTimeout(300);

    await screenshot(page, "13-chat-panel");
  });

  test("14 - Settings panel", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);

    // Open settings
    await page.locator('button[title="Settings"]').click();
    await page.waitForTimeout(300);

    await screenshot(page, "14-settings-panel");
  });

  test("15 - Network info panel", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);

    // Open network info
    await page.locator('button[title="Network info"]').click();
    await page.waitForTimeout(300);

    await screenshot(page, "15-network-info");
  });

  test("16 - Creative mode with mixed content", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await switchToCreativeMode(page);

    // Create a sticky note
    await page.locator('button[title*="sticky note"]').click();
    await page.waitForTimeout(500);

    // Type into the note
    const noteBody = page.locator(".note-body").first();
    await noteBody.click();
    await page.waitForTimeout(200);
    await noteBody.evaluate((el) => {
      const tiptap = el.querySelector(".tiptap");
      if (tiptap) {
        tiptap.innerHTML = "<p><strong>Project Ideas</strong></p><p>Build something amazing</p>";
        tiptap.dispatchEvent(new Event("input", { bubbles: true }));
      }
    });
    // Click elsewhere to deselect
    const canvas = page.locator(".absolute.inset-0.overflow-hidden.touch-none");
    await canvas.click({ position: { x: 20, y: 20 } });
    await page.waitForTimeout(200);

    // Create a text block
    await page.locator('button[title*="text block"]').click();
    const box = await canvas.boundingBox();
    await canvas.click({ position: { x: box!.width * 0.6, y: box!.height * 0.6 } });
    await page.waitForTimeout(500);

    const content = page.locator(".text-content").first();
    if (await content.isVisible()) {
      await content.click();
      await content.click();
      await content.evaluate((el) => {
        el.innerHTML = "<p>Standalone text on the canvas</p>";
        el.dispatchEvent(new Event("input", { bubbles: true }));
      });
      await page.keyboard.press("Escape");
      await page.keyboard.press("Escape");
    }
    await page.waitForTimeout(300);

    await screenshot(page, "16-creative-mixed-content");
  });

  test("17 - Font selector in sticky note", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await switchToCreativeMode(page);

    // Create a sticky note
    await page.locator('button[title*="sticky note"]').click();
    await page.waitForTimeout(500);

    const note = page.locator(".sticky-note").first();
    await expect(note).toBeVisible({ timeout: 5_000 });

    // Click the font button (Aa) in the note header to open the dropdown
    const fontBtn = note.locator('button[title="Font"]');
    await fontBtn.click();
    await page.waitForTimeout(300);

    await screenshotElement(page, ".sticky-note", "17-sticky-note-font-selector");
  });

  test("18 - Font selector in text block toolbar", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await switchToCreativeMode(page);

    // Create and select a text block
    await page.locator('button[title*="text block"]').click();
    const canvas = page.locator(".absolute.inset-0.overflow-hidden.touch-none");
    const box = await canvas.boundingBox();
    await canvas.click({ position: { x: box!.width / 2, y: box!.height / 2 } });
    await page.waitForTimeout(500);

    const content = page.locator(".text-content").first();
    await expect(content).toBeVisible({ timeout: 5_000 });
    await content.click();
    await page.waitForTimeout(200);

    // Open the font dropdown in the text block toolbar
    const wrapper = page.locator(".text-block-wrapper").first();
    const fontBtn = wrapper.locator(".font-picker-btn");
    await fontBtn.click();
    await page.waitForTimeout(300);

    await screenshotElement(page, ".text-block-wrapper", "18-text-block-font-selector");
  });

  test("19 - Context menu", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await switchToCreativeMode(page);

    // Right-click on the canvas to open context menu
    const canvas = page.locator(".absolute.inset-0.overflow-hidden.touch-none");
    const box = await canvas.boundingBox();
    await canvas.click({
      position: { x: box!.width / 2, y: box!.height / 2 },
      button: "right",
    });
    await page.waitForTimeout(300);

    await screenshot(page, "19-context-menu");
  });

  test("20 - Command palette", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);

    // Open command palette with Ctrl+K
    await page.keyboard.press("Control+k");
    await page.waitForTimeout(300);

    await screenshot(page, "20-command-palette");
  });

  test("21 - Choose name dialog", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    // Navigate but don't complete the join flow
    await page.goto(`${baseUrl}/s/${session.name}#${session.key}`);

    const joinBtn = page.locator('button:has-text("Join")');
    await joinBtn.waitFor({ timeout: 10_000 });
    await page.waitForTimeout(300);

    await screenshot(page, "21-choose-name-dialog");
  });

  test("22 - Full creative workspace", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await switchToCreativeMode(page);

    // Create multiple elements
    // Note 1
    await page.locator('button[title*="sticky note"]').click();
    await page.waitForTimeout(400);
    // Note 2
    await page.locator('button[title*="sticky note"]').click();
    await page.waitForTimeout(400);

    // Text block
    await page.locator('button[title*="text block"]').click();
    const canvas = page.locator(".absolute.inset-0.overflow-hidden.touch-none");
    const box = await canvas.boundingBox();
    await canvas.click({ position: { x: box!.width * 0.7, y: box!.height * 0.3 } });
    await page.waitForTimeout(400);

    // Enable slideshow and add a slide
    await page.locator('button[title*="Slideshow"]').click();
    await page.waitForTimeout(300);
    await page.locator('button[title="Add slide"]').click();
    await page.waitForTimeout(400);

    // Open chat
    await page.locator('button[title="Chat"]').click();
    await page.waitForTimeout(300);

    await screenshot(page, "22-full-creative-workspace");
  });
});
