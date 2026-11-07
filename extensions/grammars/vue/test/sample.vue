<!-- A to-do list: a single-file component with a template, a typed script
     and scoped styles. -->
<template>
  <section class="todo" :class="{ done: remaining === 0 }">
    <h1>{{ title.toUpperCase() }}</h1>
    <p v-if="items.length > 0">
      {{ remaining }} of {{ items.length }} left &mdash; {{ percent(remaining) }}%
    </p>
    <p v-else>Nothing to do.</p>

    <form @submit.prevent="add">
      <input v-model.trim="draft" placeholder="What needs doing?" maxlength="80" />
      <button type="submit" :disabled="draft.length === 0">Add</button>
    </form>

    <ul>
      <li
        v-for="(item, index) in items"
        :key="item.id"
        :[highlightAttr]="index === 0"
        @click="toggle(item)"
      >
        <span :style="{ opacity: item.done ? 0.5 : 1 }">{{ item.text }}</span>
        <slot name="extra" :item="item"></slot>
      </li>
    </ul>

    <TodoFooter v-slot:default="{ count }" :count="items.length">
      <a href="https://vuejs.org/guide/">Read the guide</a> ({{ count }})
    </TodoFooter>
    <template #empty><code>items.length === 0</code></template>
  </section>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import TodoFooter from "./TodoFooter.vue";

interface Item {
  id: number;
  text: string;
  done: boolean;
}

type Filter = "all" | "open" | "done";

const props = defineProps<{ title: string; filter?: Filter }>();
const emit = defineEmits<{ (event: "changed", count: number): void }>();

const highlightAttr = "data-first";
const draft = ref("");
const items = ref<Item[]>([
  { id: 1, text: "Write the \"hello\" page\n", done: false },
  { id: 2, text: 'Ship it\t✓', done: true },
]);
let nextId = 0x10;

// How many are left to do.
const remaining = computed(() => items.value.filter((item) => !item.done).length);

/* As a whole percentage of the list. */
function percent(left: number): number {
  if (items.value.length === 0) {
    return 0;
  }
  return Math.round((left / items.value.length) * 100.0);
}

function add(): void {
  const text = draft.value.trim();
  if (!text) return;
  items.value.push({ id: nextId++, text, done: false });
  draft.value = "";
  emit("changed", items.value.length);
}

async function toggle(item: Item): Promise<void> {
  item.done = !item.done;
  await Promise.resolve(props.filter ?? "all");
}
</script>

<style scoped>
/* Finished items fade out. */
.todo li {
  cursor: pointer;
  padding: 0.25rem 1em;
}

.todo.done h1::after {
  content: " \2714";
  color: #3c9a5f;
}
</style>
