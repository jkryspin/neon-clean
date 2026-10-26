#!/usr/bin/env node
/**
 * Nexon Account Creator
 * Automates creating Nexon accounts for MapleStory Classic World
 * Simulates human behavior to bypass anti-bot detection
 */

import puppeteer from 'puppeteer';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const CONFIG = {
  nexonSignupUrl: 'https://www.nexon.com/account/en/create',
  emailDomain: 'kryspin.dev',
  passwordLength: 16,
  outputDir: path.join(__dirname, '..', '..'),
  credentialsFile: 'nexon-credentials.json'
};

function parseArgs() {
  const args = process.argv.slice(2);
  const config = {
    count: 1,
    headless: true,
    delay: { min: 3000, max: 6000 },
    output: CONFIG.credentialsFile
  };

  for (let i = 0; i < args.length; i++) {
    switch (args[i]) {
      case '--count':
        config.count = parseInt(args[++i], 10);
        break;
      case '--headless':
        if (args[i + 1] === 'false') {
          config.headless = false;
          i++;
        }
        break;
      case '--no-headless':
        config.headless = false;
        break;
      case '--delay':
        if (args[i + 1]) {
          const delayParts = args[++i].split(',').map(Number);
          config.delay = { min: delayParts[0] || 3000, max: delayParts[1] || 6000 };
        }
        break;
      case '--output':
        config.output = args[++i];
        break;
    }
  }
  return config;
}

function generatePassword(length = 16) {
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*';
  let password = '';
  const randomValues = new Uint32Array(length);
  crypto.getRandomValues(randomValues);
  for (let i = 0; i < length; i++) {
    password += chars[randomValues[i] % chars.length];
  }
  return password;
}

function generateEmail() {
  const timestamp = Date.now();
  const random = Math.random().toString(36).substring(2, 8);
  return `neon_${timestamp}_${random}@${CONFIG.emailDomain}`;
}

function randomDelay(min, max) {
  return new Promise(resolve => {
    const delay = Math.floor(Math.random() * (max - min + 1)) + min;
    console.log(`  [delay] ${delay}ms`);
    setTimeout(resolve, delay);
  });
}

function delay(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

function loadCredentials(filePath) {
  try {
    if (fs.existsSync(filePath)) {
      return JSON.parse(fs.readFileSync(filePath, 'utf8'));
    }
  } catch (error) {
    console.warn(`Warning: Could not load credentials: ${error.message}`);
  }
  return { accounts: [], lastUpdated: null };
}

function saveCredentials(filePath, credentials) {
  credentials.lastUpdated = new Date().toISOString();
  fs.writeFileSync(filePath, JSON.stringify(credentials, null, 2));
  console.log(`\nCredentials saved to: ${filePath}`);
}

// Human-like mouse movement
async function humanMoveAndClick(page, x, y) {
  const startX = Math.floor(Math.random() * 100) + 50;
  const startY = Math.floor(Math.random() * 100) + 50;
  const steps = 5;

  for (let i = 1; i <= steps; i++) {
    const t = i / steps;
    const easeT = t * t * (3 - 2 * t);
    const curve = Math.sin(t * Math.PI) * 20;
    const currX = startX + (x - startX) * easeT + curve;
    const currY = startY + (y - startY) * easeT - curve;
    await page.mouse.move(currX, currY, { steps: 2 });
    await delay(50 + Math.random() * 50);
  }

  await page.mouse.click(x + (Math.random() - 0.5) * 4, y + (Math.random() - 0.5) * 4);
}

async function createAccount(browser, accountIndex, totalAccounts) {
  console.log(`\n[${accountIndex + 1}/${totalAccounts}] Creating account...`);

  const page = await browser.newPage();
  await page.setViewport({ width: 1920, height: 1080, deviceScaleFactor: 1, isMobile: false, hasTouch: false });
  await page.setUserAgent('Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36');

  const email = generateEmail();
  const password = generatePassword();
  console.log(`  Email: ${email}`);
  console.log(`  Password: ${password}`);

  try {
    console.log('  Navigating to Nexon signup page...');
    await page.goto(CONFIG.nexonSignupUrl, { waitUntil: 'domcontentloaded', timeout: 30000 });
    await delay(2000);

    // Accept cookies
    console.log('  Checking for cookie banner...');
    const hasCookieBanner = await page.evaluate(() => {
      return document.querySelector('.cookie-banner, #cookie-banner, [class*="cookie"]') !== null;
    });
    if (hasCookieBanner) {
      console.log('  Accepting cookies...');
      await page.evaluate(() => {
        const buttons = document.querySelectorAll('button');
        for (const btn of buttons) {
          const txt = btn.textContent.trim().toLowerCase();
          if (txt.includes('allow') || txt.includes('accept') || txt.includes('agree')) {
            btn.click();
            break;
          }
        }
      });
      await delay(2000);
    }

    // Set birthdate
    const birthYear = new Date().getFullYear() - (25 + Math.floor(Math.random() * 10));
    const birthMonth = Math.floor(Math.random() * 12) + 1;
    const birthDay = Math.floor(Math.random() * 28) + 1;
    console.log(`  Birthdate: ${birthMonth}/${birthDay}/${birthYear}`);

    // ============================================
    // Set dropdowns using direct JavaScript manipulation
    // ============================================
    console.log('  Setting country and date of birth...');

    // Get current state
    const initialState = await page.evaluate(() => {
      const toggles = document.querySelectorAll('.dropdown-toggle');
      return Array.from(toggles).map(t => t.textContent.trim());
    });
    console.log(`  Current: ${initialState.join(' | ')}`);

    // Function to select a dropdown value
    async function selectDropdown(dropdownIdx, value, label) {
      // Check if already set
      const currentValues = await page.evaluate(() => {
        const toggles = document.querySelectorAll('.dropdown-toggle');
        return Array.from(toggles).map(t => t.textContent.trim());
      });

      if (currentValues[dropdownIdx]?.includes(value)) {
        console.log(`    ${label} already ${value}, skipping`);
        return true;
      }

      console.log(`    Setting ${label} to ${value}...`);

      // Get the toggle element
      const toggles = await page.$$('.dropdown-toggle');
      if (dropdownIdx >= toggles.length) {
        console.log(`    ${label} toggle not found`);
        return false;
      }
      const toggle = toggles[dropdownIdx];

      // Click the toggle to open the menu
      await toggle.click();
      await delay(400);

      // Find the menu that's a sibling of this toggle's parent
      const menuElement = await page.evaluateHandle((toggleEl) => {
        const formControl = toggleEl.closest('.form-control.select');
        if (formControl) {
          const nextSibling = formControl.nextElementSibling;
          if (nextSibling && nextSibling.classList.contains('dropdown-menu')) {
            return nextSibling;
          }
        }
        const menus = document.querySelectorAll('.dropdown-menu');
        for (const menu of menus) {
          if (menu.offsetParent !== null) {
            return menu;
          }
        }
        return null;
      }, toggle);

      if (!menuElement) {
        console.log(`    Could not find menu for ${label}`);
        return false;
      }

      // Find the option within this specific menu
      const optionElement = await menuElement.$(`li[id="${value}"]`);

      if (!optionElement) {
        console.log(`    Could not find option li[id="${value}"] in menu`);
        return false;
      }

      console.log(`    Found option li[id="${value}"], clicking...`);

      // Click the option and update the component's state
      await page.evaluate((optionEl, toggleEl, value) => {
        // Get the <a> inside the <li>
        const a = optionEl.querySelector('a');

        // Click the <a> tag
        if (a) {
          a.click();
        }

        // Update the toggle's display text directly
        const btnContent = toggleEl.querySelector('.btn-content');
        if (btnContent) {
          btnContent.textContent = value;
        }

        // Remove error class if present
        toggleEl.classList.remove('error');

        // Dispatch a change event on the menu
        const menu = optionEl.parentElement;
        if (menu) {
          menu.dispatchEvent(new Event('change', { bubbles: true, composed: true }));
        }

        // Also dispatch a custom event that Vue might be listening for
        const formGroup = toggleEl.closest('.form-group');
        if (formGroup) {
          formGroup.dispatchEvent(new CustomEvent('select-change', {
            bubbles: true,
            composed: true,
            detail: { value: value }
          }));
        }
      }, optionElement, toggle, value);

      await delay(300);

      // Close the dropdown by clicking the toggle again
      await toggle.click();
      await delay(500);

      // Verify the value was set
      const newValues = await page.evaluate(() => {
        const toggles = document.querySelectorAll('.dropdown-toggle');
        return Array.from(toggles).map(t => t.textContent.trim());
      });

      if (newValues[dropdownIdx]?.includes(value)) {
        console.log(`    Verified ${label} is now ${newValues[dropdownIdx]}`);
        return true;
      } else {
        console.log(`    Warning: ${label} shows ${newValues[dropdownIdx]} instead of ${value}`);
        return false;
      }
    }

    // Select each dropdown
    await selectDropdown(0, 'United States', 'Country');
    await selectDropdown(1, String(birthMonth), 'Month');
    await selectDropdown(2, String(birthDay), 'Day');
    await selectDropdown(3, String(birthYear), 'Year');

    // Verify selections
    const finalValues = await page.evaluate(() => {
      const toggles = document.querySelectorAll('.dropdown-toggle');
      return Array.from(toggles).map(t => t.textContent.trim());
    });
    console.log(`  Selected: ${finalValues.join(' | ')}`);

    // Click Continue
    console.log('  Clicking Continue...');

    // Find the Continue button using data-cy selector
    const continueButton = await page.$('[data-cy="continue"]');

    if (continueButton) {
      console.log('  Found Continue button, clicking...');
      await continueButton.click();
    } else {
      console.log('  Continue button not found, trying alternative...');
      const buttons = await page.$$('button');
      for (const btn of buttons) {
        const txt = await btn.evaluate(b => b.textContent.trim().toUpperCase());
        if (txt === 'CONTINUE' || txt === 'NEXT') {
          console.log(`  Found button with text "${txt}", clicking...`);
          await btn.click();
          break;
        }
      }
    }

    // Wait for navigation or page change
    console.log('  Waiting for page change...');
    try {
      await page.waitForNavigation({ timeout: 5000 });
      console.log('  Page navigated!');
    } catch (e) {
      console.log('  Page did not navigate, checking for errors...');
      // Check for validation errors
      const errors = await page.evaluate(() => {
        const helpBlocks = document.querySelectorAll('.help-block');
        return Array.from(helpBlocks).map(h => ({
          text: h.textContent.trim(),
          visible: h.style.display !== 'none'
        }));
      });
      console.log('  Help blocks:', errors);

      // Check the current URL
      const url = await page.url();
      console.log('  Current URL:', url);
    }

    await randomDelay(2000, 4000);

    // ============================================
    // Step 2: Email and Password
    // ============================================
    console.log('  Step 2: Entering email and password...');
    const displayName = `NeonPlayer${Math.floor(Math.random() * 10000)}`;

    // Wait for email input
    console.log('  Waiting for email input...');
    await page.waitForFunction(
      () => document.querySelector('input[type="email"]') || document.querySelector('input[type="text"]'),
      { timeout: 20000 }
    );

    // Fill form
    await page.evaluate((email, password) => {
      const createInputEvent = (value) => new InputEvent('input', { bubbles: true, cancelable: true, data: value });
      const createChangeEvent = () => new Event('change', { bubbles: true, cancelable: true });

      // Email
      const emailInput = document.querySelector('input[type="email"]') || document.querySelector('input[name*="email"]') || document.querySelector('input[type="text"]');
      if (emailInput) {
        emailInput.focus();
        emailInput.value = email;
        emailInput.dispatchEvent(createInputEvent(email));
        emailInput.dispatchEvent(createChangeEvent());
      }

      // Passwords
      const passwordInputs = document.querySelectorAll('input[type="password"]');
      if (passwordInputs.length >= 1) {
        passwordInputs[0].focus();
        passwordInputs[0].value = password;
        passwordInputs[0].dispatchEvent(createInputEvent(password));
        passwordInputs[0].dispatchEvent(createChangeEvent());
      }
      if (passwordInputs.length >= 2) {
        passwordInputs[1].focus();
        passwordInputs[1].value = password;
        passwordInputs[1].dispatchEvent(createInputEvent(password));
        passwordInputs[1].dispatchEvent(createChangeEvent());
      }

      // Checkboxes
      document.querySelectorAll('input[type="checkbox"]').forEach(cb => {
        if (!cb.checked) {
          cb.checked = true;
          cb.dispatchEvent(new Event('change', { bubbles: true }));
        }
      });
    }, email, password);

    await randomDelay(1500, 3000);

    // Submit
    console.log('  Submitting form...');
    await page.evaluate(() => {
      const buttons = document.querySelectorAll('button');
      for (const btn of buttons) {
        const txt = btn.textContent.trim().toUpperCase();
        if (txt.includes('CREATE') || txt.includes('CONTINUE') || txt.includes('SIGN UP') || txt.includes('NEXT')) {
          btn.click();
          break;
        }
      }
    });

    await randomDelay(5000, 8000);

    // Check result
    console.log('  Checking result...');
    const result = await page.evaluate(() => {
      const content = document.body.textContent || '';
      const url = window.location.href;
      const title = document.title || '';

      const successPatterns = [/account.*created/i, /successfully/i, /welcome/i, /verification.*sent/i, /check.*your.*email/i, /please.*verify/i, /confirm.*email/i, /email.*sent/i];
      const errorPatterns = [/error/i, /failed/i, /already.*exist/i, /invalid/i, /required/i, /please.*enter/i, /must.*be/i];

      let status = 'pending_verification';
      for (const pattern of successPatterns) {
        if (pattern.test(content)) { status = 'created'; break; }
      }
      for (const pattern of errorPatterns) {
        if (pattern.test(content) && status !== 'created') { status = 'error'; break; }
      }

      return { status, url, title, contentPreview: content.substring(0, 1000) };
    });

    console.log(`  Status: ${result.status}`);
    console.log(`  URL: ${result.url}`);

    // Save screenshot and HTML
    const safeEmail = email.replace(/[^a-z0-9]/g, '_');
    const logsDir = path.join(CONFIG.outputDir, 'logs');
    if (!fs.existsSync(logsDir)) fs.mkdirSync(logsDir, { recursive: true });

    const screenshotPath = path.join(logsDir, `signup_${safeEmail}.png`);
    await page.screenshot({ path: screenshotPath, fullPage: true });
    console.log(`  Screenshot: ${screenshotPath}`);

    const htmlPath = path.join(logsDir, `signup_${safeEmail}.html`);
    fs.writeFileSync(htmlPath, await page.content());
    console.log(`  HTML: ${htmlPath}`);

    return { email, password, displayName, status: result.status, url: result.url, createdAt: new Date().toISOString(), screenshot: screenshotPath, html: htmlPath };

  } catch (error) {
    console.error(`  Error: ${error.message}`);
    try {
      const safeEmail = email.replace(/[^a-z0-9]/g, '_');
      await page.screenshot({ path: path.join(CONFIG.outputDir, 'logs', `error_${safeEmail}.png`), fullPage: true });
    } catch (e) {}
    return { email, password, status: 'error', error: error.message, createdAt: new Date().toISOString() };
  } finally {
    await page.close();
  }
}

async function main() {
  const args = parseArgs();

  console.log('='.repeat(60));
  console.log('Nexon Account Creator - MapleStory Classic World');
  console.log('='.repeat(60));
  console.log(`\nConfiguration:`);
  console.log(`  Accounts to create: ${args.count}`);
  console.log(`  Headless mode: ${args.headless ? 'yes' : 'no'}`);
  console.log(`  Email domain: ${CONFIG.emailDomain}`);
  console.log(`  Delay range: ${args.delay.min}-${args.delay.max}ms`);

  const credentialsPath = path.join(CONFIG.outputDir, args.output);
  const credentials = loadCredentials(credentialsPath);
  const existingEmails = new Set(credentials.accounts.map(a => a.email));
  console.log(`\nExisting accounts: ${credentials.accounts.length}`);

  console.log('\nLaunching browser...');
  let browserExecutable = process.env.BROWSER_EXECUTABLE || '/usr/bin/chromium' || '/usr/bin/chromium-browser' || '/usr/bin/google-chrome';
  if (!fs.existsSync(browserExecutable)) browserExecutable = undefined;
  console.log(`  Using browser: ${browserExecutable || 'default'}`);

  const browser = await puppeteer.launch({
    executablePath: browserExecutable || undefined,
    headless: args.headless,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu', '--disable-features=VizDisplayCompositor', '--disable-blink-features=AutomationControlled', '--window-size=1920,1080']
  });

  try {
    const newAccounts = [];

    for (let i = 0; i < args.count; i++) {
      const account = await createAccount(browser, i, args.count);
      newAccounts.push(account);

      if (!existingEmails.has(account.email)) {
        credentials.accounts.push(account);
        existingEmails.add(account.email);
      }

      if (i < args.count - 1) {
        console.log(`\nDelay before next account...`);
        await randomDelay(args.delay.min * 2, args.delay.max * 2);
      }
    }

    saveCredentials(credentialsPath, credentials);

    console.log('\n' + '='.repeat(60));
    console.log('SUMMARY');
    console.log('='.repeat(60));
    console.log(`Total created: ${newAccounts.length}`);
    console.log(`  - Created: ${newAccounts.filter(a => a.status === 'created').length}`);
    console.log(`  - Pending: ${newAccounts.filter(a => a.status === 'pending_verification').length}`);
    console.log(`  - Errors: ${newAccounts.filter(a => a.status === 'error').length}`);
    console.log(`\nTotal in file: ${credentials.accounts.length}`);

    console.log('\n' + '-'.repeat(60));
    console.log('NEW ACCOUNTS:');
    console.log('-'.repeat(60));
    for (const account of newAccounts) {
      console.log(`\nEmail: ${account.email}`);
      console.log(`Password: ${account.password}`);
      console.log(`Status: ${account.status}`);
      if (account.displayName) console.log(`Display Name: ${account.displayName}`);
      if (account.error) console.log(`Error: ${account.error}`);
    }

  } finally {
    await browser.close();
  }
}

main().catch(error => {
  console.error('Fatal error:', error);
  process.exit(1);
});
