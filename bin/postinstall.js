#!/usr/bin/env node
'use strict';

const orange = '\x1b[38;5;208m';
const slate = '\x1b[38;5;244m';
const green = '\x1b[1;32m';
const bold = '\x1b[1m';
const reset = '\x1b[0m';

console.log('');
console.log(`${orange}░█▀▀░█░░░█▀█░█░█░█▀▄░█▀▀${slate}░░░░░█░█░█▀▀░█▀▀░█▀▄${reset}`);
console.log(`${orange}░█░░░█░░░█▀█░█░█░█░█░█▀▀${slate}░▄▄▄░█░█░▀▀█░█▀▀░█▀▄${reset}`);
console.log(`${orange}░▀▀▀░▀▀▀░▀░▀░▀▀▀░▀▀░░▀▀▀${slate}░░░░░▀▀▀░▀▀▀░▀▀▀░▀░▀${reset}\n`);

const pkg = require('../package.json');
console.log(`${green}✓ Installed claude-user v${pkg.version} successfully!${reset}`);
console.log(`Run ${bold}cuser${reset} to get started switching profiles.\n`);
