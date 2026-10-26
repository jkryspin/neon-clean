#!/usr/bin/env node
/**
 * Connect to existing Firefox via Remote Debugging and watch signup
 */

import { firefox } from 'playwright';

async function main() {
  console.log('Connecting to Firefox via Remote Debugging (localhost:6000)...');

  // Connect to Firefox via WebSocket
  const browser = await firefox.connectOverCDP('ws://localhost:6000/debugger');

  const pages = await browser.pages();
  console.log(`Found ${pages.length} pages/tabs`);

  for (let i = 0; i < pages.length; i++) {
    const page = pages[i];
    const url = await page.url();
    console.log(`  ${i + 1}. ${url.substring(0, 80)}${url.length > 80 ? '...' : ''}`);
  }

  // Find the signup page or use the first page
  let page = pages.find(p => p.url().includes('nexon.com/account'));
  if (!page) {
    page = pages[0];
    console.log('\nSignup page not found, using first tab.');
    console.log('Please navigate to https://www.nexon.com/account/en/create\n');
  }

  console.log('\n=== Connected to Firefox ===\n');
  console.log('Current page:', await page.url());

  // Log initial state
  await logPageState(page);

  // Set up mutation observer to log DOM changes
  await page.evaluate(() => {
    const observer = new MutationObserver((mutations) => {
      mutations.forEach((mutation) => {
        if (mutation.type === 'attributes') {
          const target = mutation.target;
          if (target.classList && target.classList.contains('dropdown-toggle')) {
            console.log(`[DOM] Dropdown toggle changed: ${target.textContent.trim()}`);
          }
          if (target.classList && target.classList.contains('help-block')) {
            console.log(`[DOM] Help block style changed: ${target.style.display}`);
          }
        }
        if (mutation.type === 'childList' && mutation.addedNodes.length > 0) {
          mutation.addedNodes.forEach((node) => {
            if (node.nodeType === 1) {
              const classes = node.classList;
              if (classes && classes.contains('dropdown-menu')) {
                console.log('[DOM] Dropdown menu added');
              }
            }
          });
        }
      });
    });

    observer.observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['class', 'style']
    });

    window.signupObserver = observer;
  });

  console.log('\n=== Monitoring page changes ===\n');
  console.log('Please interact with the page. The script will log changes.\n');
  console.log('Press Ctrl+C to stop.\n');

  // Keep running
  await new Promise(() => {});
}

async function logPageState(page) {
  const state = await page.evaluate(() => {
    const toggles = document.querySelectorAll('.dropdown-toggle');
    const toggleValues = Array.from(toggles).map(t => ({
      text: t.textContent.trim(),
      hasError: t.classList.contains('error')
    }));

    const helpBlocks = document.querySelectorAll('.help-block');
    const helpMessages = Array.from(helpBlocks).map(h => ({
      text: h.textContent.trim(),
      visible: h.style.display !== 'none'
    }));

    const url = window.location.href;

    return { toggleValues, helpMessages, url };
  });

  console.log('Current state:');
  console.log('  URL:', state.url);
  console.log('  Dropdown values:', state.toggleValues);
  console.log('  Help messages:', state.helpMessages);
  console.log();
}

main().catch(error => {
  console.error('Error:', error);
  process.exit(1);
});
