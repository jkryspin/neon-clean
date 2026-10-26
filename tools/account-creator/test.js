#!/usr/bin/env node
import puppeteer from 'puppeteer';

const browser = await puppeteer.launch({
  headless: true,
  args: [
    '--no-sandbox',
    '--disable-setuid-sandbox',
    '--disable-dev-shm-usage',
    '--disable-gpu',
    '--disable-features=VizDisplayCompositor',
    '--disable-blink-features=AutomationControlled'
  ]
});

const page = await browser.newPage();

// Enable console logging
page.on('console', msg => console.log('PAGE CONSOLE:', msg.text()));
page.on('pageerror', err => console.log('PAGE ERROR:', err.message));

// Set up request interception to see redirects
page.on('request', req => console.log('REQUEST:', req.url()));
page.on('response', res => console.log('RESPONSE:', res.url(), res.status()));

await page.setViewport({ width: 1920, height: 1080 });
await page.setUserAgent('Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36');

console.log('\n=== Navigating to Nexon signup ===\n');

try {
  await page.goto('https://www.nexon.com/account/en/create', {
    waitUntil: 'domcontentloaded',
    timeout: 60000
  });

  console.log('\n=== Page loaded ===\n');
  console.log('URL:', page.url());
  console.log('Title:', await page.title());

  // Wait a bit more for dynamic content
  await new Promise(r => setTimeout(r, 5000));

  // Get page content
  const content = await page.content();
  console.log('\n=== First 2000 chars of content ===\n');
  console.log(content.substring(0, 2000));

  // Look for form elements
  const form = await page.$('form');
  console.log('\nForm found:', !!form);

  const inputs = await page.$$('input');
  console.log('Inputs found:', inputs.length);
  for (const input of inputs) {
    const type = await page.evaluate(el => el.type, input);
    const id = await page.evaluate(el => el.id, input);
    const name = await page.evaluate(el => el.name, input);
    console.log(`  - type:${type} id:${id} name:${name}`);
  }

  // Take screenshot
  await page.screenshot({ path: '/home/john/neon/logs/nexon_signup_debug.png', fullPage: true });
  console.log('\nScreenshot saved to /home/john/neon/logs/nexon_signup_debug.png');

} catch (error) {
  console.error('Error:', error.message);
  try {
    await page.screenshot({ path: '/home/john/neon/logs/nexon_signup_error.png', fullPage: true });
    console.log('Error screenshot saved');
  } catch (e) {}
} finally {
  await browser.close();
}
