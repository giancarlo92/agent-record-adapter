// Part of Agent Record Adapter (see NOTICE).
'use strict';

// A list to tick in the terminal: arrows move, space ticks, a ticks everything, enter confirms.

const readline = require('readline');

function multiselect(title, items, out = process.stdout, input = process.stdin) {
  return new Promise((resolve, reject) => {
    let cursor = 0;
    const ticked = items.map((item) => Boolean(item.checked));
    let drawn = 0;

    const draw = () => {
      if (drawn) out.write(`\x1b[${drawn}A\x1b[0J`);
      const lines = [title, '  (arrows to move, space to tick, a for all, enter to confirm)'];
      items.forEach((item, i) => {
        const box = ticked[i] ? '[x]' : '[ ]';
        const pointer = i === cursor ? '>' : ' ';
        const note = item.note ? `  ${item.note}` : '';
        lines.push(` ${pointer} ${box} ${item.label}${note}`);
      });
      out.write(lines.join('\n') + '\n');
      drawn = lines.length;
    };

    readline.emitKeypressEvents(input);
    if (input.isTTY) input.setRawMode(true);
    const done = (value, error) => {
      input.removeListener('keypress', onKey);
      if (input.isTTY) input.setRawMode(false);
      input.pause();
      if (error) reject(error);
      else resolve(value);
    };
    const onKey = (_text, key = {}) => {
      if (key.ctrl && key.name === 'c') return done(null, new Error('Cancelled'));
      if (key.name === 'up') cursor = (cursor + items.length - 1) % items.length;
      else if (key.name === 'down') cursor = (cursor + 1) % items.length;
      else if (key.name === 'space') ticked[cursor] = !ticked[cursor];
      else if (key.name === 'a') {
        const all = ticked.every(Boolean);
        ticked.fill(!all);
      } else if (key.name === 'return') return done(items.filter((_, i) => ticked[i]).map((item) => item.value));
      draw();
    };
    input.on('keypress', onKey);
    input.resume();
    draw();
  });
}

function confirm(question, fallback, out = process.stdout, input = process.stdin) {
  return new Promise((resolve) => {
    const rl = readline.createInterface({ input, output: out });
    rl.question(`${question} ${fallback ? '(Y/n)' : '(y/N)'} `, (answer) => {
      rl.close();
      const a = answer.trim().toLowerCase();
      resolve(a ? a.startsWith('y') || a.startsWith('s') : fallback);
    });
  });
}

module.exports = { multiselect, confirm };
