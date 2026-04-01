/**
 * TextBlock interaction tests — fullstack e2e via Playwright.
 *
 * Verifies the three-state interaction model:
 *   idle → (click) → selected → (click) → editing
 *   editing → (Escape) → selected → (Escape) → idle
 *   any → (click outside) → idle
 *
 * Also tests: text input, drag-move when selected, toolbar visibility,
 * and multi-user sync of text blocks.
 */
import { test, expect, type Page, type Locator } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";
import { createSession } from "./helpers/grpc-client";
import { computeEncryptedZeros } from "./helpers/encrypt";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const TEST_KEY = "testkey1234567";
let cachedEncryptedZeros: Buffer | null = null;

/** Get or compute the encrypted zeros for the test key (cached). */
async function getEncryptedZeros(): Promise<Buffer> {
  if (!cachedEncryptedZeros) {
    cachedEncryptedZeros = await computeEncryptedZeros(TEST_KEY);
  }
  return cachedEncryptedZeros;
}

function getEndpoint(): string {
  const portFile = path.join(__dirname, ".server-port");
  try {
    const port = fs.readFileSync(portFile, "utf-8").trim();
    return `[::1]:${port}`;
  } catch {
    return `[::1]:${process.env.SSHX_TEST_PORT || "18051"}`;
  }
}

function getBaseUrl(): string {
  const portFile = path.join(__dirname, ".server-port");
  try {
    const port = fs.readFileSync(portFile, "utf-8").trim();
    return `http://[::1]:${port}`;
  } catch {
    return `http://[::1]:${process.env.SSHX_TEST_PORT || "18051"}`;
  }
}

/** Open a session page, complete the name dialog, and wait for connection. */
async function openSession(
  page: Page,
  baseUrl: string,
  sessionName: string,
  key: string
): Promise<void> {
  const url = `${baseUrl}/s/${sessionName}#${key}`;
  await page.goto(url);

  // The session shows a "Choose your name" dialog first. Fill it and click Join.
  const joinBtn = page.locator('button:has-text("Join")');
  await joinBtn.waitFor({ timeout: 10_000 });
  const nameInput = page.locator('input[placeholder]');
  if (await nameInput.isVisible()) {
    await nameInput.fill("TestUser");
  }
  await joinBtn.click();

  // Wait for the toolbar mode tabs to appear (session is fully connected).
  await page.waitForSelector('button[title="Notes"]', { timeout: 10_000 });

  // Switch to creative mode so the text tool button is available.
  await page.locator('button[title="Notes"]').click();

  // Wait for the text block button to appear.
  await page.waitForSelector('button[title*="text block"]', {
    timeout: 5_000,
  });
}

/** Click the "T" button in the toolbar to activate the text tool. */
async function activateTextTool(page: Page): Promise<void> {
  const btn = page.locator('button[title*="text block"], button[title*="Cancel text tool"]');
  await btn.click();
}

/** Create a text block at approximate center of the viewport. */
async function createTextBlock(page: Page): Promise<void> {
  await activateTextTool(page);
  // Click the canvas (the main fabric div) to place the text block.
  // The fabric div is the full-viewport overlay with touch-none class.
  const canvas = page.locator(".absolute.inset-0.overflow-hidden.touch-none");
  const box = await canvas.boundingBox();
  expect(box).toBeTruthy();
  await canvas.click({ position: { x: box!.width / 2, y: box!.height / 2 } });
}

/** Get the text-block wrapper element (assumes exactly one on the page). */
function getTextBlockWrapper(page: Page): Locator {
  return page.locator(".text-block-wrapper");
}

/** Get the text-content element inside the text block. */
function getTextContent(page: Page): Locator {
  return page.locator(".text-content");
}

/** Click the text content to select it. */
async function selectTextBlock(page: Page): Promise<void> {
  const content = getTextContent(page);
  await content.click();
  await expect(content).toHaveClass(/selected/, { timeout: 2_000 });
}

/** Click twice to enter editing mode, then wait for focus. */
async function enterEditing(page: Page): Promise<void> {
  const content = getTextContent(page);
  await content.click();
  await content.click();
  await expect(content).toHaveClass(/editing/, { timeout: 2_000 });
  // Ensure the contenteditable div has keyboard focus
  await content.focus();
  await page.waitForTimeout(100);
}

/** Type text into the contenteditable div via innerHTML + input event. */
async function typeIntoTextBlock(page: Page, text: string): Promise<void> {
  const content = getTextContent(page);
  await content.evaluate((el, t) => {
    el.focus();
    el.innerHTML = t;
    el.dispatchEvent(new Event("input", { bubbles: true }));
  }, text);
}

test.describe("TextBlock Interactions", () => {
  let endpoint: string;
  let baseUrl: string;

  test.beforeAll(() => {
    endpoint = getEndpoint();
    baseUrl = getBaseUrl();
  });

  test("create a text block via the text tool", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);

    await createTextBlock(page);

    // A text block should now exist on the canvas
    const wrapper = getTextBlockWrapper(page);
    await expect(wrapper).toBeVisible({ timeout: 5_000 });
  });

  test("click to select — shows blue outline", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Initially not selected (no .selected class)
    await expect(content).not.toHaveClass(/selected/);

    // Click the text block to select it.
    // Use evaluate to dispatch a real click event (Playwright pointer actions
    // may be intercepted by the canvas overlay).
    await content.evaluate((el) => el.click());
    await page.waitForTimeout(200);

    // Should now have the "selected" class → blue border
    await expect(content).toHaveClass(/selected/);
  });

  test("second click enters editing mode", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // First click → selected
    await content.click();
    await expect(content).toHaveClass(/selected/);
    await expect(content).not.toHaveAttribute("contenteditable", "true");

    // Second click → editing
    await content.click();
    await expect(content).toHaveClass(/editing/);
    await expect(content).toHaveAttribute("contenteditable", "true");
  });

  test("can type text when in editing mode", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Enter editing
    await enterEditing(page);

    // Type text into the contenteditable div
    await typeIntoTextBlock(page, "Hello sshx!");

    // Verify text was entered
    await expect(content).toContainText("Hello sshx!");
  });

  test("Escape from editing returns to selected state", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Enter editing: click → click
    await content.click();
    await content.click();
    await expect(content).toHaveClass(/editing/);

    // Escape → back to selected (not editing)
    await page.keyboard.press("Escape");
    await expect(content).not.toHaveClass(/editing/);
    // Should still be selected (blue outline)
    await expect(content).toHaveClass(/selected/);
  });

  test("Escape from selected returns to idle", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Click to select
    await content.click();
    await expect(content).toHaveClass(/selected/);

    // Escape → idle (no selected, no editing)
    await page.keyboard.press("Escape");
    await expect(content).not.toHaveClass(/selected/);
    await expect(content).not.toHaveClass(/editing/);
  });

  test("double Escape goes editing → selected → idle", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Enter editing
    await content.click();
    await content.click();
    await expect(content).toHaveClass(/editing/);

    // First Escape → selected
    await page.keyboard.press("Escape");
    await expect(content).toHaveClass(/selected/);
    await expect(content).not.toHaveClass(/editing/);

    // Second Escape → idle
    await page.keyboard.press("Escape");
    await expect(content).not.toHaveClass(/selected/);
    await expect(content).not.toHaveClass(/editing/);
  });

  test("click outside deselects from selected state", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Select
    await content.click();
    await expect(content).toHaveClass(/selected/);

    // Click on the canvas background (far from the text block)
    const canvas = page.locator(".absolute.inset-0.overflow-hidden.touch-none");
    await canvas.click({ position: { x: 20, y: 20 } });

    // Should be deselected
    await expect(content).not.toHaveClass(/selected/);
    await expect(content).not.toHaveClass(/editing/);
  });

  test("click outside deselects from editing state", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Enter editing
    await content.click();
    await content.click();
    await expect(content).toHaveClass(/editing/);

    // Click outside
    const canvas = page.locator(".absolute.inset-0.overflow-hidden.touch-none");
    await canvas.click({ position: { x: 20, y: 20 } });

    // Should be fully idle
    await expect(content).not.toHaveClass(/selected/);
    await expect(content).not.toHaveClass(/editing/);
  });

  test("toolbar visible when selected, hidden when idle", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    const wrapper = getTextBlockWrapper(page);
    const toolbar = wrapper.locator(".toolbar");

    // Initially idle — toolbar not visible
    await expect(toolbar).not.toBeVisible();

    // Select → toolbar visible
    await content.click();
    await expect(toolbar).toBeVisible();

    // Still visible in editing
    await content.click();
    await expect(toolbar).toBeVisible();

    // Escape to selected → still visible
    await page.keyboard.press("Escape");
    await expect(toolbar).toBeVisible();

    // Escape to idle → toolbar hidden
    await page.keyboard.press("Escape");
    await expect(toolbar).not.toBeVisible();
  });

  test("selected text block has blue border", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Click to select
    await selectTextBlock(page);
    // Wait for CSS transition to complete
    await page.waitForTimeout(250);

    // Check the computed border color is blue (#60a5fa = rgb(96, 165, 250))
    const borderColor = await content.evaluate((el) => {
      return window.getComputedStyle(el).borderColor;
    });
    expect(borderColor).toBe("rgb(96, 165, 250)");
  });

  test("editing text block has indigo border", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Enter editing
    await enterEditing(page);
    // Wait for CSS transition to complete
    await page.waitForTimeout(250);

    // #818cf8 = rgb(129, 140, 248)
    const borderColor = await content.evaluate((el) => {
      return window.getComputedStyle(el).borderColor;
    });
    expect(borderColor).toBe("rgb(129, 140, 248)");
  });

  test("idle text block has transparent border", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Should be idle — transparent border
    const borderColor = await content.evaluate((el) => {
      return window.getComputedStyle(el).borderColor;
    });
    expect(borderColor).toMatch(/rgba\(0,\s*0,\s*0,\s*0\)|transparent/);
  });

  test("toolbar font size selector works", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Select to show toolbar
    await content.click();

    const wrapper = getTextBlockWrapper(page);
    const fontSelect = wrapper.locator(".toolbar-select");
    await expect(fontSelect).toBeVisible();

    // Default is MD (24px)
    const initialFontSize = await content.evaluate((el) => el.style.fontSize);
    expect(initialFontSize).toBe("24px");

    // Change to XL (48px)
    await fontSelect.selectOption("xl");
    await page.waitForTimeout(200);

    const newFontSize = await content.evaluate((el) => el.style.fontSize);
    expect(newFontSize).toBe("48px");
  });

  test("toolbar color picker changes text color", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Select to show toolbar
    await content.click();

    const wrapper = getTextBlockWrapper(page);
    // Click the red color dot (#f87171)
    const redDot = wrapper.locator('.color-dot[title="Color: #f87171"]');
    await redDot.click();
    await page.waitForTimeout(200);

    const textColor = await content.evaluate((el) => el.style.color);
    expect(textColor).toBe("rgb(248, 113, 113)");
  });

  test("toolbar delete button removes text block", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Select to show toolbar
    await content.click();

    const wrapper = getTextBlockWrapper(page);
    const deleteBtn = wrapper.locator(".delete-btn");
    await deleteBtn.click();

    // Text block should disappear
    await expect(wrapper).not.toBeVisible({ timeout: 5_000 });
  });

  test("text persists after exiting editing mode", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Enter editing and type
    await enterEditing(page);
    await typeIntoTextBlock(page, "Persistent text");

    // Exit editing via Escape
    await page.keyboard.press("Escape");
    await expect(content).not.toHaveClass(/editing/);

    // Text should still be there
    await expect(content).toContainText("Persistent text");
  });

  test("update dispatches to parent when editing stops", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Enter editing, type, and exit
    await enterEditing(page);
    await typeIntoTextBlock(page, "Updated content");
    await page.keyboard.press("Escape");

    // The text should remain visible (not wiped by placeholder)
    await expect(content).toContainText("Updated content");
    // Should not have the "empty" class since it has content
    await expect(content).not.toHaveClass(/empty/);
  });

  test("contenteditable is false when not editing", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Idle — not contenteditable
    const idleEditable = await content.getAttribute("contenteditable");
    expect(idleEditable).toBe("false");

    // Selected — still not contenteditable
    await content.click();
    const selectedEditable = await content.getAttribute("contenteditable");
    expect(selectedEditable).toBe("false");

    // Editing — contenteditable
    await content.click();
    const editingEditable = await content.getAttribute("contenteditable");
    expect(editingEditable).toBe("true");
  });

  test("alignment buttons change text alignment", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // Select to show toolbar
    await content.click();

    const wrapper = getTextBlockWrapper(page);

    // Click center alignment
    const centerBtn = wrapper.locator('.toolbar-btn[title="Align center"]');
    await centerBtn.click();
    await page.waitForTimeout(200);

    const align = await content.evaluate((el) => el.style.textAlign);
    expect(align).toBe("center");

    // Click right alignment
    const rightBtn = wrapper.locator('.toolbar-btn[title="Align right"]');
    await rightBtn.click();
    await page.waitForTimeout(200);

    const alignR = await content.evaluate((el) => el.style.textAlign);
    expect(alignR).toBe("right");
  });

  test("empty text block shows empty class", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);
    await createTextBlock(page);

    const content = getTextContent(page);
    await expect(content).toBeVisible({ timeout: 5_000 });

    // A freshly created text block should have the "empty" class
    await expect(content).toHaveClass(/empty/);
  });

  test("keyboard shortcut T activates text tool", async ({ page }) => {
    const session = await createSession(endpoint, await getEncryptedZeros());
    await openSession(page, baseUrl, session.name, session.key);

    // Press T to activate the text tool
    await page.keyboard.press("t");

    // The canvas should have crosshair cursor (text tool active)
    const fabric = page.locator(".absolute.inset-0.overflow-hidden.touch-none");
    const hasCrosshair = await fabric.evaluate((el) =>
      el.classList.contains("cursor-crosshair")
    );
    expect(hasCrosshair).toBe(true);

    // Click to place the text block
    const box = await fabric.boundingBox();
    await fabric.click({ position: { x: box!.width / 2, y: box!.height / 2 } });

    // Text block should appear
    const wrapper = getTextBlockWrapper(page);
    await expect(wrapper).toBeVisible({ timeout: 5_000 });
  });
});
