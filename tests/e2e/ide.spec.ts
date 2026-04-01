/**
 * E2E tests for the IDE editor feature (OpenVSCode Server iframe widget).
 *
 * Tests verify that the IDE toolbar button state and widget creation work
 * correctly based on whether an IDE-capable CLI client is connected.
 */
import { test, expect, type Page } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";
import { createSession, simulateIdeCli } from "./helpers/grpc-client";
import { computeEncryptedZeros } from "./helpers/encrypt";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const TEST_KEY = "testkey1234567";
let cachedEncryptedZeros: Buffer | null = null;

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
  await page.waitForSelector('button[title="New terminal"]', { timeout: 10_000 });
}

test("IDE button disabled when no IDE client connected", async ({ page }) => {
  const endpoint = getEndpoint();
  const baseUrl = getBaseUrl();
  const zeros = await getEncryptedZeros();

  const { name, key } = await createSession(endpoint, zeros);

  await openSession(page, baseUrl, name, key);

  // The IDE button title contains "IDE not available" when no CLI with IDE is connected
  const ideButton = page.locator(
    'button[title="IDE not available (start sshx with --ide to enable)"]'
  );
  await ideButton.waitFor({ timeout: 5_000 });
  await expect(ideButton).toBeDisabled();
});

test("IDE button enabled after IDE CLI connects", async ({ page }) => {
  const endpoint = getEndpoint();
  const baseUrl = getBaseUrl();
  const zeros = await getEncryptedZeros();

  const { name, token, key } = await createSession(endpoint, zeros);

  // Simulate an IDE-capable CLI connection before opening the session page
  const stopIdeCli = simulateIdeCli(endpoint, name, token);

  try {
    await openSession(page, baseUrl, name, key);

    // Wait briefly for the IdeAvailable WS message to propagate
    await page.waitForTimeout(500);

    // The button should now have the "VS Code editors" title and be enabled
    const ideButton = page.locator('button[title="VS Code editors"]');
    await ideButton.waitFor({ timeout: 5_000 });
    await expect(ideButton).not.toBeDisabled();
  } finally {
    stopIdeCli();
  }
});

test("IDE not shown at start, user requests IDE, new IDE widget displayed", async ({
  page,
}) => {
  const endpoint = getEndpoint();
  const baseUrl = getBaseUrl();
  const zeros = await getEncryptedZeros();

  const { name, token, key } = await createSession(endpoint, zeros);

  const stopIdeCli = simulateIdeCli(endpoint, name, token);

  try {
    await openSession(page, baseUrl, name, key);

    // Wait for IDE available to propagate
    await page.waitForTimeout(500);

    // No iframe with IDE src should exist yet
    const existingIframe = page.locator(`iframe[src*="/ide/s/"]`);
    await expect(existingIframe).toHaveCount(0);

    // Click the IDE toolbar button to open dropdown
    const ideButton = page.locator('button[title="VS Code editors"]');
    await ideButton.waitFor({ timeout: 5_000 });
    await ideButton.click();

    // Click "New editor" in the dropdown
    const newEditorButton = page.locator('button:has-text("New editor")');
    await newEditorButton.waitFor({ timeout: 3_000 });
    await newEditorButton.click();

    // Wait for the IDE widget iframe to appear in the DOM
    // The iframe loads /ide/s/{session}/{ide_id}/ — may return 503 (no real IDE),
    // but the widget element itself should exist and be visible
    const ideIframe = page.locator(`iframe[src*="/ide/s/${name}/"]`);
    await ideIframe.waitFor({ timeout: 10_000 });
    await expect(ideIframe).toBeVisible();
  } finally {
    stopIdeCli();
  }
});
