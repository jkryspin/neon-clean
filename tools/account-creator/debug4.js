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

// Examine dropdown structure
const dropdownInfo = await page.evaluate(() => {
  const dropdowns = document.querySelectorAll('.form-control.select.dropdown-toggle');
  const info = [];

  dropdowns.forEach((dd, i) => {
    info.push({
      index: i,
      text: dd.textContent.trim(),
      className: dd.className,
      parentClass: dd.parentElement.className,
      grandparentClass: dd.parentElement.parentElement?.className
    });

    // Check for parent form-group
    const formGroup = dd.closest('.form-group');
    if (formGroup) {
      const children = Array.from(formGroup.children).map(c => ({
        tag: c.tagName,
        class: c.className,
        text: c.textContent.trim().substring(0, 50)
      }));
      info.push({ parentChildren: children });
    }
  });

  return info;
});

console.log('Dropdown info:', JSON.stringify(dropdownInfo, null, 2));

// Click month dropdown and examine options
console.log('\n--- Clicking month dropdown ---');
await dropdowns[1].click();
await new Promise(r => setTimeout(r, 1500));

// Get all divs that might be options
const optionsInfo = await page.evaluate(() => {
  const allDivs = Array.from(document.querySelectorAll('div')).map((d, i) => ({
    index: i,
    class: d.className,
    text: d.textContent.trim(),
    style: d.style.display
  })).filter(d => d.text && d.text.length < 20);

  // Look for dropdown menu
  const dropdownMenus = Array.from(document.querySelectorAll('.dropdown-menu, .options, [class*="dropdown"]')).map(d => ({
    class: d.className,
    children: Array.from(d.children).map(c => ({
      text: c.textContent.trim(),
      class: c.className
    }))
  }));

  return { allDivs: allDivs.slice(0, 50), dropdownMenus };
});

console.log('Options info:', JSON.stringify(optionsInfo, null, 2));

// Take screenshot
await page.screenshot({ path: '/home/john/neon/logs/debug4.png', fullPage: true });
console.log('Screenshot saved');

await browser.close();
