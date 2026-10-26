#!/usr/bin/env node
import puppeteer from 'puppeteer';

const browser = await puppeteer.launch({
  headless: true,
  args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
});

const page = await browser.newPage();
await page.setViewport({ width: 1920, height: 1080 });

console.log('Navigating to Nexon signup...\n');

await page.goto('https://www.nexon.com/account/en/create', {
  waitUntil: 'domcontentloaded',
  timeout: 30000
});

await new Promise(r => setTimeout(r, 5000));

// Accept cookies
await page.evaluate(() => {
  const btn = Array.from(document.querySelectorAll('button')).find(b => b.textContent.trim() === 'Allow All');
  if (btn) { btn.scrollIntoView(); btn.click(); }
});
await new Promise(r => setTimeout(r, 2000));

// Get dropdowns
const dropdowns = await page.$$('.form-control.select.dropdown-toggle');
console.log(`Found ${dropdowns.length} dropdowns`);

// Click month dropdown
console.log('Clicking month dropdown...');
await dropdowns[1].click();
await new Promise(r => setTimeout(r, 1000));

// Click an option
await page.evaluate(() => {
  const options = document.querySelectorAll('.dropdown-menu div, .options div');
  console.log('Options found:', options.length);
  for (const opt of options) {
    console.log('  Option:', opt.textContent.trim());
    if (opt.textContent.trim() === '01') {
      opt.click();
      console.log('Clicked 01');
      break;
    }
  }
});
await new Promise(r => setTimeout(r, 1000));

// Take screenshot
await page.screenshot({ path: '/home/john/neon/logs/debug3_step1.png', fullPage: true });
console.log('Screenshot after month selection saved');

// Click Continue
await page.evaluate(() => {
  const btn = Array.from(document.querySelectorAll('button')).find(b => b.textContent.trim() === 'CONTINUE');
  if (btn) {
    console.log('Found Continue button, clicking...');
    btn.scrollIntoView();
    btn.click();
  } else {
    console.log('Continue button not found');
  }
});
await new Promise(r => setTimeout(r, 5000));

// Check what's on the page now
const url = page.url();
const title = await page.title();
console.log(`\nAfter Continue: URL=${url}, Title=${title}`);

// Check for email input
const hasEmailInput = await page.$('input[type="email"]');
console.log('Email input found:', !!hasEmailInput);

// Get all inputs
const inputs = await page.evaluate(() => {
  return Array.from(document.querySelectorAll('input')).map(el => ({
    type: el.type,
    name: el.name,
    id: el.id
  }));
});
console.log('All inputs:', JSON.stringify(inputs, null, 2));

// Take screenshot
await page.screenshot({ path: '/home/john/neon/logs/debug3_step2.png', fullPage: true });
console.log('Screenshot after Continue saved');

await browser.close();
