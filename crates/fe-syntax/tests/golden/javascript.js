// A tiny command palette, used as a golden highlighting sample.
'use strict';

import { fuzzyScore } from './fuzzy.js';
import * as React from 'react';

const MAX_RESULTS = 12;
const commands = new Map();

/**
 * Registers a command under a unique id.
 * @param {string} id
 * @param {() => void} run
 */
export function register(id, title, run) {
  if (commands.has(id)) {
    throw new Error(`command ${id} is already registered`);
  }
  commands.set(id, { id, title, run, uses: 0 });
}

export class Palette extends EventTarget {
  #query = '';
  static instances = 0;

  constructor(element) {
    super();
    this.element = element ?? document.body;
    Palette.instances += 1;
  }

  get query() {
    return this.#query;
  }

  set query(value) {
    this.#query = value.trim().toLowerCase();
    this.dispatchEvent(new CustomEvent('change', { detail: this.results() }));
  }

  results() {
    const scored = [...commands.values()]
      .map((command) => ({ command, score: fuzzyScore(this.#query, command.title) }))
      .filter(({ score }) => score > 0);
    scored.sort((a, b) => b.score - a.score || b.command.uses - a.command.uses);
    return scored.slice(0, MAX_RESULTS).map(({ command }) => command);
  }

  async run(id) {
    const command = commands.get(id);
    if (!command) return false;
    command.uses++;
    await Promise.resolve(command.run());
    return true;
  }
}

export function PaletteList({ items, selected, onPick }) {
  return (
    <ul className="palette" role="listbox">
      {items.map((item, index) => (
        <li key={item.id} aria-selected={index === selected} onClick={() => onPick(item.id)}>
          {item.title}
        </li>
      ))}
    </ul>
  );
}

const isMac = /Mac|iPhone/.test(globalThis.navigator?.platform ?? '');
register('palette.open', 'Open Palette', () => console.log(isMac ? '⌘P' : 'Ctrl+P', 0.5, null, undefined));
