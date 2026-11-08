<script lang="ts">
  // A to-do list: runes, a typed store and an async loader.
  import { onMount } from 'svelte';
  import type { Snippet } from 'svelte';

  interface Todo {
    id: number;
    title: string;
    done: boolean;
  }

  type Filter = 'all' | 'open' | 'done';

  let { heading = 'Things to do', footer }: { heading?: string; footer?: Snippet } = $props();

  let todos: Todo[] = $state([]);
  let filter: Filter = $state('all');
  let draft = $state('');
  const MAX_TODOS = 0x40;

  /* Derived values recompute when their inputs change. */
  let visible = $derived(
    todos.filter((todo) => filter === 'all' || (filter === 'done') === todo.done),
  );

  function add(title: string): void {
    if (!title.trim() || todos.length >= MAX_TODOS) return;
    todos.push({ id: Date.now(), title: title.trim(), done: false });
    draft = '';
  }

  async function load(url: string): Promise<Todo[]> {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`HTTP ${response.status}\n`);
    return (await response.json()) as Todo[];
  }

  let loading = load('/api/todos?limit=25');

  onMount(() => console.log("mounted, with \"quotes\" and a tab:\t", 1.5e3));
</script>

<!-- The page: a form, the list and its footer. -->
<svelte:head>
  <title>{heading} &mdash; Todos</title>
</svelte:head>

{#snippet row(todo: Todo, index: number)}
  <li class:done={todo.done}>
    <input type="checkbox" bind:checked={todo.done} />
    <span>{index + 1}. {todo.title}</span>
  </li>
{/snippet}

<h1>{heading}</h1>

<form onsubmit={(event) => { event.preventDefault(); add(draft); }}>
  <input bind:value={draft} placeholder="What needs doing?" maxlength="80" />
  <button type="submit" disabled={!draft}>Add</button>
</form>

{#await loading}
  <p>Loading&hellip;</p>
{:then loaded}
  <p>{loaded.length} saved, {todos.length} here.</p>
{:catch error}
  <p class="error">{error.message}</p>
{/await}

{#if visible.length === 0}
  <p>Nothing to show.</p>
{:else if visible.length > 10}
  <p>That's a long list.</p>
{:else}
  <ul>
    {#each visible as todo, index (todo.id)}
      {@const label = `#${todo.id}`}
      {@render row(todo, index)}
      <small title={label}>{label}</small>
    {/each}
  </ul>
{/if}

{#key filter}
  <p>Showing <code>{filter}</code> todos.</p>
{/key}

{@html '<em>Rendered</em> as HTML'}
{@debug todos, filter}

{#if footer}{@render footer()}{/if}

<a href="https://svelte.dev/docs">Svelte docs</a>

<style>
  /* Scoped to this component. */
  h1 {
    font-size: 2rem;
    color: #ff3e00;
  }

  li.done span {
    text-decoration: line-through;
    opacity: 0.6;
  }

  .error::before {
    content: "\26A0  ";
  }
</style>
