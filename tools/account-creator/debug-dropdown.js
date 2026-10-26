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

await new Promise(r => setTimeout(r, 3000));

// Accept cookies
await page.evaluate(() => {
  const btn = Array.from(document.querySelectorAll('button')).find(b =>
    b.textContent.trim().toLowerCase().includes('allow') ||
    b.textContent.trim().toLowerCase().includes('accept')
  );
  if (btn) { btn.scrollIntoView(); btn.click(); }
});
await new Promise(r => setTimeout(r, 2000));

// Get detailed dropdown structure
const dropdownStructure = await page.evaluate(() => {
  const result = {};

  // Find all dropdown toggles
  const toggles = document.querySelectorAll('.dropdown-toggle');
  console.log(`Found ${toggles.length} dropdown toggles`);

  toggles.forEach((toggle, idx) => {
    const parent = toggle.closest('.form-group') || toggle.closest('.btn-group');
    const menu = parent?.querySelector('.dropdown-menu') || toggle.nextElementSibling?.querySelector('.dropdown-menu');

    result[`dropdown_${idx}`] = {
      toggleText: toggle.textContent.trim(),
      toggleClass: toggle.className,
      hasMenu: !!menu,
      menuClass: menu?.className,
      menuStyle: menu?.style.cssText,
      menuDisplay: menu?.style.display,
      menuOptions: []
    };

    if (menu) {
      const lis = menu.querySelectorAll('li');
      console.log(`Dropdown ${idx} has ${lis.length} options`);

      lis.forEach((li, liIdx) => {
        if (liIdx < 15) { // Only log first 15
          result[`dropdown_${idx}`].menuOptions.push({
            text: li.textContent.trim(),
            className: li.className,
            id: li.id,
            hasA: !!li.querySelector('a'),
            hasButton: !!li.querySelector('button')
          });
        }
      });
    }
  });

  return result;
});

console.log('Dropdown structure:');
console.log(JSON.stringify(dropdownStructure, null, 2));

// Now try to click the Month dropdown and see what happens
console.log('\n--- Clicking Month dropdown ---');
const monthToggle = await page.$('.dropdown-toggle:nth-of-type(2)');
if (monthToggle) {
  await monthToggle.click();
  await new Promise(r => setTimeout(r, 1000));

  // Check state after click
  const afterClick = await page.evaluate(() => {
    const toggles = document.querySelectorAll('.dropdown-toggle');
    const menus = document.querySelectorAll('.dropdown-menu');

    return {
      toggle2Text: toggles[1]?.textContent.trim(),
      menu2Display: menus[1]?.style.display,
      menu2Class: menus[1]?.className,
      menu2Visible: menus[1]?.offsetParent !== null,
      allMenus: Array.from(menus).map((m, i) => ({
        index: i,
        display: m.style.display,
        className: m.className,
        visible: m.offsetParent !== null
      }))
    };
  });

  console.log('After clicking month dropdown:');
  console.log(JSON.stringify(afterClick, null, 2));

  // Take screenshot
  await page.screenshot({ path: '/home/john/neon/logs/debug_dropdown_open.png', fullPage: true });
  console.log('Screenshot saved');
}

await browser.close();
