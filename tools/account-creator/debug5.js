#!/usr/bin/env node
import puppeteer from 'puppeteer';

const browser = await puppeteer.launch({
  headless: true,
  args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
});

const page = await browser.newPage();
await page.setViewport({ width: 1920, height: 1080 });

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

// Set month
console.log('Setting month...');
await dropdowns[1].click();
await new Promise(r => setTimeout(r, 800));
await page.evaluate(() => {
  const lis = document.querySelectorAll('.dropdown-menu li');
  for (const li of lis) {
    if (li.textContent.trim() === '01') {
      console.log('Found and clicking month 01');
      li.click();
      break;
    }
  }
});
await new Promise(r => setTimeout(r, 800));

// Set day
console.log('Setting day...');
await dropdowns[2].click();
await new Promise(r => setTimeout(r, 800));
await page.evaluate(() => {
  const lis = document.querySelectorAll('.dropdown-menu li');
  for (const li of lis) {
    if (li.textContent.trim() === '15') {
      console.log('Found and clicking day 15');
      li.click();
      break;
    }
  }
});
await new Promise(r => setTimeout(r, 800));

// Set year
console.log('Setting year...');
await dropdowns[3].click();
await new Promise(r => setTimeout(r, 800));
await page.evaluate(() => {
  const lis = document.querySelectorAll('.dropdown-menu li');
  for (const li of lis) {
    if (li.textContent.trim() === '1998') {
      console.log('Found and clicking year 1998');
      li.click();
      break;
    }
  }
});
await new Promise(r => setTimeout(r, 800));

// Check what the dropdowns show now
const dropdownValues = await page.evaluate(() => {
  const dropdowns = document.querySelectorAll('.form-control.select.dropdown-toggle');
  return Array.from(dropdowns).map(d => d.textContent.trim());
});
console.log('Dropdown values after selection:', dropdownValues);

// Take screenshot
await page.screenshot({ path: '/home/john/neon/logs/debug5_before.png', fullPage: true });
console.log('Screenshot before Continue saved');

// Click Continue
console.log('Clicking Continue...');
await page.evaluate(() => {
  const btn = Array.from(document.querySelectorAll('button')).find(b => b.textContent.trim() === 'CONTINUE');
  if (btn) {
    console.log('Found Continue button');
    btn.scrollIntoView();
    btn.click();
    console.log('Clicked Continue');
  } else {
    console.log('Continue button not found');
  }
});
await new Promise(r => setTimeout(r, 5000));

// Check what's on the page now
const url = page.url();
const title = await page.title();
console.log(`\nAfter Continue: URL=${url}, Title=${title}`);

// Get all buttons
const buttons = await page.evaluate(() => {
  return Array.from(document.querySelectorAll('button')).map(b => b.textContent.trim());
});
console.log('Buttons:', buttons);

// Check for email input
const hasEmailInput = await page.$('input[type="email"]');
console.log('Email input found:', !!hasEmailInput);

// Take screenshot
await page.screenshot({ path: '/home/john/neon/logs/debug5_after.png', fullPage: true });
console.log('Screenshot after Continue saved');

await browser.close();
