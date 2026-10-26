#!/usr/bin/env node
import puppeteer from 'puppeteer';

const browser = await puppeteer.launch({
  headless: true,
  args: [
    '--no-sandbox',
    '--disable-setuid-sandbox',
    '--disable-dev-shm-usage',
    '--disable-gpu',
    '--disable-features=VizDisplayCompositor'
  ]
});

const page = await browser.newPage();
await page.setViewport({ width: 1920, height: 1080 });

console.log('Navigating to Nexon signup...\n');

await page.goto('https://www.nexon.com/account/en/create', {
  waitUntil: 'domcontentloaded',
  timeout: 30000
});

// Wait for page to stabilize
await new Promise(r => setTimeout(r, 5000));

console.log('URL:', page.url());
console.log('Title:', await page.title());

// Get all form elements
const formElements = await page.evaluate(() => {
  const result = {
    selects: [],
    inputs: [],
    buttons: [],
    divs: []
  };

  document.querySelectorAll('select').forEach(el => {
    result.selects.push({
      name: el.name,
      id: el.id,
      options: Array.from(el.options).map(o => o.text).slice(0, 5)
    });
  });

  document.querySelectorAll('input').forEach(el => {
    result.inputs.push({
      type: el.type,
      name: el.name,
      id: el.id,
      placeholder: el.placeholder
    });
  });

  document.querySelectorAll('button').forEach(el => {
    result.buttons.push({
      text: el.textContent.trim(),
      type: el.type,
      id: el.id
    });
  });

  // Get main container classes
  document.querySelectorAll('div[class*="create"], div[class*="form"], form').forEach(el => {
    result.divs.push({
      class: el.className,
      tag: el.tagName
    });
  });

  return result;
});

console.log('\n=== SELECTS ===');
console.log(JSON.stringify(formElements.selects, null, 2));

console.log('\n=== INPUTS ===');
console.log(JSON.stringify(formElements.inputs, null, 2));

console.log('\n=== BUTTONS ===');
console.log(JSON.stringify(formElements.buttons, null, 2));

console.log('\n=== FORM CONTAINERS ===');
console.log(JSON.stringify(formElements.divs, null, 2));

// Take screenshot
await page.screenshot({ path: '/home/john/neon/logs/debug2.png', fullPage: true });
console.log('\nScreenshot saved to /home/john/neon/logs/debug2.png');

await browser.close();
