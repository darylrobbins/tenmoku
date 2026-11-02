#include "tree_sitter/api.h"
#include "./parser.h"
#include <stddef.h>
#include <stdint.h>

#ifdef TREE_SITTER_FEATURE_WASM

#include "./alloc.h"
#include "./array.h"
#include "./atomic.h"
#include "./language.h"
#include "./lexer.h"
#include "./wasm-stdlib/external_scanner_stdlib.h"
#include "./wasm_store.h"

#include <string.h>
#include <wasm.h>
#include <wasmtime.h>

#ifdef _MSC_VER
#pragma warning(push)
#pragma warning(disable : 4100)
#elif defined(__GNUC__) || defined(__clang__)
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wunused-parameter"
#endif

#define array_len(a) (sizeof(a) / sizeof(a[0]))

// fastedit patch: whether the Rust binding's check for this thread
// (`tree_sitter::with_wasm_interrupt`) says to stop the running Wasm code.
// Defined in `binding_rust/wasm_language.rs`.
extern bool ts_wasm_thread_interrupted(void);

// fastedit patch: called each time a store reaches its epoch deadline, one
// epoch after the last, while Wasm code runs. Code the thread's check stops
// fails with an error, which the callers below report as a failed call;
// otherwise it runs on to the next epoch.
static wasmtime_error_t *ts_wasm_store__epoch_deadline(
  wasmtime_context_t *context,
  void *data,
  uint64_t *epoch_deadline_delta,
  wasmtime_update_deadline_kind_t *update_kind
) {
  if (ts_wasm_thread_interrupted()) {
    return wasmtime_error_new("Wasm code stopped at its deadline");
  }
  *epoch_deadline_delta = 1;
  *update_kind = WASMTIME_UPDATE_DEADLINE_CONTINUE;
  return NULL;
}

// The following symbols from the C and C++ standard libraries are available
// for external scanners to use.
const char *STDLIB_SYMBOLS[] = {
  #include "./wasm-stdlib/imports.txt"
};

// The contents of the `dylink.0` custom section of a Wasm module,
// as specified by the current WebAssembly dynamic linking ABI proposal.
typedef struct {
  uint32_t memory_size;
  uint32_t memory_align;
  uint32_t table_size;
  uint32_t table_align;
} WasmDylinkInfo;

// WasmLanguageId - A pointer used to identify a language. This language id is
// reference-counted, so that its ownership can be shared between the language
// itself and the instances of the language that are held in Wasm stores.
typedef struct {
  volatile uint32_t ref_count;
  volatile uint32_t is_language_deleted;
} WasmLanguageId;

// TSWasmLanguage - A reference-counted language loaded from a Wasm module.
// This data is read-only and does not reference a particular Wasm store, so it
// can be shared by all users of the language.
typedef struct {
  TSLanguage language;
  volatile uint32_t ref_count;
  WasmLanguageId *language_id;
  wasmtime_module_t *module;
  const char *name;
  char *symbol_name_buffer;
  char *field_name_buffer;
  WasmDylinkInfo dylink_info;
} TSWasmLanguage;

static inline TSWasmLanguage *ts_language__wasm_language(const TSLanguage *self) {
  return (TSWasmLanguage *)((char *)self - offsetof(TSWasmLanguage, language));
}

// LanguageWasmInstance - Additional data associated with an instantiation of
// a `TSLanguage` in a particular Wasm store. The Wasm store holds one of
// these structs for each language that it has instantiated.
typedef struct {
  WasmLanguageId *language_id;
  wasmtime_instance_t instance;
  int32_t external_states_address;
  wasmtime_func_t lex_main_fn;
  wasmtime_func_t lex_keyword_fn;
  wasmtime_func_t scanner_create_fn;
  wasmtime_func_t scanner_destroy_fn;
  wasmtime_func_t scanner_serialize_fn;
  wasmtime_func_t scanner_deserialize_fn;
  wasmtime_func_t scanner_scan_fn;
} LanguageWasmInstance;

typedef struct {
  wasmtime_func_t reset_heap;
  wasmtime_func_t proc_exit;
  wasmtime_func_t abort;
  wasmtime_func_t assert_fail;
  wasmtime_func_t notify_memory_growth;
  wasmtime_func_t debug_message;
  wasmtime_func_t at_exit;
  wasmtime_func_t args_get;
  wasmtime_func_t args_sizes_get;
} BuiltinFunctionIndices;

// TSWasmStore - A struct that allows a given `Parser` to use Wasm-backed
// languages. This struct is mutable, and can only be used by one parser at a
// time.
struct TSWasmStore {
  wasm_engine_t *engine;
  wasmtime_store_t *store;
  wasmtime_table_t function_table;
  wasmtime_memory_t memory;
  TSLexer *current_lexer;
  LanguageWasmInstance *current_instance;
  Array(LanguageWasmInstance) language_instances;
  uint32_t current_memory_offset;
  uint32_t current_function_table_offset;
  wasmtime_func_t *stdlib_fn_indices;
  BuiltinFunctionIndices builtin_fn_indices;
  wasmtime_global_t stack_pointer_global;
  wasm_globaltype_t *const_i32_type;
  bool has_error;
  uint32_t lexer_address;
  // fastedit patch: the language being lexed, whose token counts bound the
  // symbols its lexers and scanner may return.
  const TSLanguage *current_language;
  // fastedit patch: the stack pointer once the store is set up, put back by
  // `ts_wasm_store_reset_heap`: a call stopped part way (a trap, or the
  // deadline) leaves it lowered.
  wasmtime_val_t stack_pointer_start;
};

typedef Array(char) StringData;

// LanguageInWasmMemory - The memory layout of a `TSLanguage` when compiled to
// wasm32. This is used to copy static language data out of the Wasm memory.
typedef struct {
  uint32_t abi_version;
  uint32_t symbol_count;
  uint32_t alias_count;
  uint32_t token_count;
  uint32_t external_token_count;
  uint32_t state_count;
  uint32_t large_state_count;
  uint32_t production_id_count;
  uint32_t field_count;
  uint16_t max_alias_sequence_length;
  int32_t parse_table;
  int32_t small_parse_table;
  int32_t small_parse_table_map;
  int32_t parse_actions;
  int32_t symbol_names;
  int32_t field_names;
  int32_t field_map_slices;
  int32_t field_map_entries;
  int32_t symbol_metadata;
  int32_t public_symbol_map;
  int32_t alias_map;
  int32_t alias_sequences;
  int32_t lex_modes;
  int32_t lex_fn;
  int32_t keyword_lex_fn;
  TSSymbol keyword_capture_token;
  struct {
    int32_t states;
    int32_t symbol_map;
    int32_t create;
    int32_t destroy;
    int32_t scan;
    int32_t serialize;
    int32_t deserialize;
  } external_scanner;
  int32_t primary_state_ids;
  int32_t name;
  int32_t reserved_words;
  uint16_t max_reserved_word_set_size;
  uint32_t supertype_count;
  int32_t supertype_symbols;
  int32_t supertype_map_slices;
  int32_t supertype_map_entries;
  TSLanguageMetadata metadata;
} LanguageInWasmMemory;

// LexerInWasmMemory - The memory layout of a `TSLexer` when compiled to wasm32.
// This is used to copy mutable lexing state in and out of the Wasm memory.
typedef struct {
  int32_t lookahead;
  TSSymbol result_symbol;
  int32_t advance;
  int32_t mark_end;
  int32_t get_column;
  int32_t is_at_included_range_start;
  int32_t eof;
} LexerInWasmMemory;

// Linear memory layout:
// [ <-- stack | stdlib statics | lexer | language statics --> | serialization_buffer | heap --> ]
#define MAX_MEMORY_SIZE (128 * 1024 * 1024 / MEMORY_PAGE_SIZE)

/************************
 * WasmDylinkMemoryInfo
 ***********************/

typedef struct {
  const uint8_t *data;
  size_t offset;
  size_t size;
} WasmReader;

static bool wasm_reader__read_u8(WasmReader *reader, uint8_t *result) {
  if (reader->offset >= reader->size) return false;
  *result = reader->data[reader->offset++];
  return true;
}

static bool wasm_reader__read_uleb128(WasmReader *reader, uint32_t *result) {
  uint32_t value = 0;
  for (unsigned shift = 0; shift < 32; shift += 7) {
    uint8_t byte;
    if (!wasm_reader__read_u8(reader, &byte)) return false;
    if (shift == 28 && (byte & 0xf0) != 0) return false;
    value |= (uint32_t)(byte & 0x7f) << shift;
    if ((byte & 0x80) == 0) {
      *result = value;
      return true;
    }
  }
  return false;
}

static bool wasm_dylink_info__parse(
  const uint8_t *bytes,
  size_t length,
  WasmDylinkInfo *info
) {
  const uint8_t WASM_MAGIC_NUMBER[4] = {0, 'a', 's', 'm'};
  const uint8_t WASM_VERSION[4] = {1, 0, 0, 0};
  const uint8_t WASM_CUSTOM_SECTION = 0x0;
  const uint8_t WASM_DYLINK_MEM_INFO = 0x1;

  if (length < 8) return false;
  if (memcmp(bytes, WASM_MAGIC_NUMBER, 4) != 0) return false;
  if (memcmp(bytes + 4, WASM_VERSION, 4) != 0) return false;

  WasmReader reader = {
    .data = bytes,
    .offset = 8,
    .size = length,
  };

  while (reader.offset < reader.size) {
    uint8_t section_id;
    uint32_t section_length;
    if (
      !wasm_reader__read_u8(&reader, &section_id) ||
      !wasm_reader__read_uleb128(&reader, &section_length) ||
      section_length > reader.size - reader.offset
    ) return false;
    size_t section_end = reader.offset + section_length;

    if (section_id == WASM_CUSTOM_SECTION) {
      size_t previous_size = reader.size;
      reader.size = section_end;
      uint32_t name_length;
      if (
        !wasm_reader__read_uleb128(&reader, &name_length) ||
        name_length > reader.size - reader.offset
      ) return false;
      size_t name_end = reader.offset + name_length;

      if (name_length == 8 && memcmp(&reader.data[reader.offset], "dylink.0", 8) == 0) {
        reader.offset = name_end;
        while (reader.offset < section_end) {
          uint8_t subsection_type;
          uint32_t subsection_size;
          if (
            !wasm_reader__read_u8(&reader, &subsection_type) ||
            !wasm_reader__read_uleb128(&reader, &subsection_size) ||
            subsection_size > section_end - reader.offset
          ) return false;
          size_t subsection_end = reader.offset + subsection_size;
          if (subsection_type == WASM_DYLINK_MEM_INFO) {
            reader.size = subsection_end;
            if (
              !wasm_reader__read_uleb128(&reader, &info->memory_size) ||
              !wasm_reader__read_uleb128(&reader, &info->memory_align) ||
              !wasm_reader__read_uleb128(&reader, &info->table_size) ||
              !wasm_reader__read_uleb128(&reader, &info->table_align) ||
              reader.offset != subsection_end
            ) return false;
            return true;
          }
          reader.offset = subsection_end;
        }
      }
      reader.size = previous_size;
    }
    reader.offset = section_end;
  }
  return false;
}

/*******************************************
 * Native callbacks exposed to Wasm modules
 *******************************************/

static wasm_trap_t *callback__abort(
  void *env,
  wasmtime_caller_t* caller,
  wasmtime_val_raw_t *args_and_results,
  size_t args_and_results_len
) {
  return wasmtime_trap_new("Wasm module called abort", 24);
}

static wasm_trap_t *callback__debug_message(
  void *env,
  wasmtime_caller_t* caller,
  wasmtime_val_raw_t *args_and_results,
  size_t args_and_results_len
) {
  wasmtime_context_t *context = wasmtime_caller_context(caller);
  TSWasmStore *store = env;
  ts_assert(args_and_results_len == 2);
  uint32_t string_address = args_and_results[0].i32;
  uint32_t value = args_and_results[1].i32;
  uint8_t *memory = wasmtime_memory_data(context, &store->memory);
  // fastedit patch: the string must end inside the module's memory.
  size_t memory_size = wasmtime_memory_data_size(context, &store->memory);
  if (
    string_address >= memory_size ||
    !memchr(&memory[string_address], 0, memory_size - string_address)
  ) {
    const char *message = "debug message outside Wasm memory";
    return wasmtime_trap_new(message, strlen(message));
  }
  printf("DEBUG: %s %u\n", &memory[string_address], value);
  return NULL;
}

static wasm_trap_t *callback__noop(
  void *env,
  wasmtime_caller_t* caller,
  wasmtime_val_raw_t *args_and_results,
  size_t args_and_results_len
) {
  return NULL;
}

// fastedit patch: the lexer callbacks trap when no lexer is running, as when a
// module's initializer calls them, instead of dereferencing a null lexer.
static wasm_trap_t *ts_wasm_store__no_lexer(void) {
  const char *message = "lexer function called outside a lex";
  return wasmtime_trap_new(message, strlen(message));
}

static wasm_trap_t *callback__lexer_advance(
  void *env,
  wasmtime_caller_t* caller,
  wasmtime_val_raw_t *args_and_results,
  size_t args_and_results_len
) {
  wasmtime_context_t *context = wasmtime_caller_context(caller);
  ts_assert(args_and_results_len == 2);

  TSWasmStore *store = env;
  TSLexer *lexer = store->current_lexer;
  if (!lexer) return ts_wasm_store__no_lexer();
  bool skip = args_and_results[1].i32;
  lexer->advance(lexer, skip);

  uint8_t *memory = wasmtime_memory_data(context, &store->memory);
  memcpy(&memory[store->lexer_address], &lexer->lookahead, sizeof(lexer->lookahead));
  return NULL;
}

static wasm_trap_t *callback__lexer_mark_end(
  void *env,
  wasmtime_caller_t* caller,
  wasmtime_val_raw_t *args_and_results,
  size_t args_and_results_len
) {
  TSWasmStore *store = env;
  TSLexer *lexer = store->current_lexer;
  if (!lexer) return ts_wasm_store__no_lexer();
  lexer->mark_end(lexer);
  return NULL;
}

static wasm_trap_t *callback__lexer_get_column(
  void *env,
  wasmtime_caller_t* caller,
  wasmtime_val_raw_t *args_and_results,
  size_t args_and_results_len
) {
  TSWasmStore *store = env;
  TSLexer *lexer = store->current_lexer;
  if (!lexer) return ts_wasm_store__no_lexer();
  uint32_t result = lexer->get_column(lexer);
  args_and_results[0].i32 = result;
  return NULL;
}

static wasm_trap_t *callback__lexer_is_at_included_range_start(
  void *env,
  wasmtime_caller_t* caller,
  wasmtime_val_raw_t *args_and_results,
  size_t args_and_results_len
) {
  TSWasmStore *store = env;
  TSLexer *lexer = store->current_lexer;
  if (!lexer) return ts_wasm_store__no_lexer();
  bool result = lexer->is_at_included_range_start(lexer);
  args_and_results[0].i32 = result;
  return NULL;
}

static wasm_trap_t *callback__lexer_eof(
  void *env,
  wasmtime_caller_t* caller,
  wasmtime_val_raw_t *args_and_results,
  size_t args_and_results_len
) {
  TSWasmStore *store = env;
  TSLexer *lexer = store->current_lexer;
  if (!lexer) return ts_wasm_store__no_lexer();
  bool result = lexer->eof(lexer);
  args_and_results[0].i32 = result;
  return NULL;
}

typedef struct {
  void *storage_location;
  wasmtime_func_unchecked_callback_t callback;
  wasm_functype_t *type;
} FunctionDefinition;

typedef struct {
  const uint8_t *data;
  size_t size;
} WasmMemory;

static bool wasm_memory__contains(const WasmMemory *memory, int32_t address, size_t size) {
  if (address < 0) return false;
  size_t start = (size_t)address;
  return start <= memory->size && size <= memory->size - start;
}

static bool wasm_memory__read(const WasmMemory *memory, int32_t address, void *result, size_t size) {
  if (!wasm_memory__contains(memory, address, size)) return false;
  memcpy(result, &memory->data[address], size);
  return true;
}

static bool wasm_memory__string_length(const WasmMemory *memory, int32_t address, size_t *length) {
  if (address < 0 || (size_t)address >= memory->size) return false;
  const uint8_t *data = &memory->data[address];
  size_t limit = memory->size - (size_t)address;
  for (size_t i = 0; i < limit; i++) {
    if (data[i] == 0) {
      *length = i;
      return true;
    }
  }
  return false;
}

static void *copy(const WasmMemory *memory, int32_t address, size_t size, bool *ok) {
  if (!*ok || size == 0) return NULL;
  if (!wasm_memory__contains(memory, address, size)) {
    *ok = false;
    return NULL;
  }
  void *result = ts_malloc(size);
  memcpy(result, &memory->data[address], size);
  return result;
}

// fastedit patch: the size of the table at `start_address`, which is the
// distance to the next table's address, or 0 if it has none.
static size_t unsized_static_array_size(
  int32_t start_address,
  const int32_t all_addresses[],
  size_t address_count
) {
  if (start_address <= 0) return 0;
  int32_t end_address = 0;
  for (unsigned i = 0; i < address_count; i++) {
    if (all_addresses[i] > start_address) {
      if (!end_address || all_addresses[i] < end_address) {
        end_address = all_addresses[i];
      }
    }
  }
  return end_address ? (size_t)end_address - (size_t)start_address : 0;
}

static void *copy_unsized_static_array(
  const WasmMemory *memory,
  int32_t start_address,
  const int32_t all_addresses[],
  size_t address_count,
  size_t *copied_size,
  bool *ok
) {
  // fastedit patch: the copy's size, for the table validation to bound the
  // table by.
  *copied_size = 0;
  if (!*ok || start_address == 0) return NULL;
  if (start_address < 0) {
    *ok = false;
    return NULL;
  }

  int32_t end_address = 0;
  for (unsigned i = 0; i < address_count; i++) {
    if (all_addresses[i] > start_address) {
      if (!end_address || all_addresses[i] < end_address) {
        end_address = all_addresses[i];
      }
    }
  }

  if (!end_address) {
    *ok = false;
    return NULL;
  }
  size_t size = (size_t)end_address - (size_t)start_address;
  if (!wasm_memory__contains(memory, start_address, size)) {
    *ok = false;
    return NULL;
  }
  void *result = ts_malloc(size);
  memcpy(result, &memory->data[start_address], size);
  *copied_size = size;
  return result;
}

static void *copy_strings(
  const WasmMemory *memory,
  int32_t array_address,
  size_t count,
  StringData *string_data,
  bool *ok
) {
  if (!*ok) return NULL;
  if (count > SIZE_MAX / sizeof(char *)) {
    *ok = false;
    return NULL;
  }
  if (count > (SIZE_MAX / sizeof(int32_t)) ||
      !wasm_memory__contains(memory, array_address, count * sizeof(int32_t))) {
    *ok = false;
    return NULL;
  }

  const char **result = ts_malloc(count * sizeof(char *));
  for (unsigned i = 0; i < count; i++) {
    int32_t address;
    memcpy(&address, &memory->data[array_address + i * sizeof(address)], sizeof(address));
    if (address == 0) {
      result[i] = (const char *)-1;
    } else {
      size_t len;
      if (!wasm_memory__string_length(memory, address, &len) || len > UINT32_MAX) {
        ts_free(result);
        *ok = false;
        return NULL;
      }
      result[i] = (const char *)(uintptr_t)string_data->size;
      array_extend(string_data, len + 1, &memory->data[address]);
    }
  }
  for (unsigned i = 0; i < count; i++) {
    if (result[i] == (const char *)-1) {
      result[i] = NULL;
    } else {
      result[i] = string_data->contents + (uintptr_t)result[i];
    }
  }
  return result;
}

static void *copy_string(
  const WasmMemory *memory,
  int32_t address,
  bool *ok
) {
  if (!*ok) return NULL;
  size_t len;
  if (!wasm_memory__string_length(memory, address, &len)) {
    *ok = false;
    return NULL;
  }
  const char *string = (const char *)&memory->data[address];
  char *result = ts_malloc(len + 1);
  memcpy(result, string, len + 1);
  return result;
}

static void delete_partially_loaded_language(
  TSWasmLanguage *result,
  StringData *symbol_name_buffer,
  StringData *field_name_buffer
) {
  if (result) {
    TSLanguage *language = &result->language;
    ts_free((void *)language->alias_map);
    ts_free((void *)language->alias_sequences);
    ts_free((void *)language->external_scanner.symbol_map);
    ts_free((void *)language->field_map_entries);
    ts_free((void *)language->field_map_slices);
    ts_free((void *)language->field_names);
    ts_free((void *)language->lex_modes);
    ts_free((void *)language->name);
    ts_free((void *)language->parse_actions);
    ts_free((void *)language->parse_table);
    ts_free((void *)language->primary_state_ids);
    ts_free((void *)language->public_symbol_map);
    ts_free((void *)language->reserved_words);
    ts_free((void *)language->small_parse_table);
    ts_free((void *)language->small_parse_table_map);
    ts_free((void *)language->supertype_map_entries);
    ts_free((void *)language->supertype_map_slices);
    ts_free((void *)language->supertype_symbols);
    ts_free((void *)language->symbol_metadata);
    ts_free((void *)language->symbol_names);
    ts_free(result);
  }
  array_delete(symbol_name_buffer);
  array_delete(field_name_buffer);
}

static bool name_eq(const wasm_name_t *name, const char *string) {
  size_t length = strlen(string);
  return name->size == length && memcmp(name->data, string, length) == 0;
}

static inline wasm_functype_t* wasm_functype_new_4_0(
  wasm_valtype_t* p1,
  wasm_valtype_t* p2,
  wasm_valtype_t* p3,
  wasm_valtype_t* p4
) {
  wasm_valtype_t* ps[4] = {p1, p2, p3, p4};
  wasm_valtype_vec_t params, results;
  wasm_valtype_vec_new(&params, 4, ps);
  wasm_valtype_vec_new_empty(&results);
  return wasm_functype_new(&params, &results);
}

#define format(output, ...) \
  do { \
    size_t message_length = snprintf((char *)NULL, 0, __VA_ARGS__); \
    *output = ts_malloc(message_length + 1); \
    snprintf(*output, message_length + 1, __VA_ARGS__); \
  } while (0)

WasmLanguageId *language_id_new(void) {
  WasmLanguageId *self = ts_malloc(sizeof(WasmLanguageId));
  self->is_language_deleted = false;
  self->ref_count = 1;
  return self;
}

WasmLanguageId *language_id_clone(WasmLanguageId *self) {
  atomic_inc(&self->ref_count);
  return self;
}

void language_id_delete(WasmLanguageId *self) {
  if (atomic_dec(&self->ref_count) == 0) {
    ts_free(self);
  }
}

static wasmtime_extern_t get_builtin_extern(
  wasmtime_func_t *func
) {
  return (wasmtime_extern_t) {
    .kind = WASMTIME_EXTERN_FUNC,
    .of.func = *func
  };
}

static bool ts_wasm_store__provide_builtin_import(
  TSWasmStore *self,
  const wasm_name_t *import_name,
  wasmtime_extern_t *import
) {
  wasmtime_error_t *error = NULL;
  wasmtime_context_t *context = wasmtime_store_context(self->store);

  // Dynamic linking parameters
  if (name_eq(import_name, "__memory_base")) {
    wasmtime_val_t value = WASM_I32_VAL(self->current_memory_offset);
    wasmtime_global_t global;
    error = wasmtime_global_new(context, self->const_i32_type, &value, &global);
    ts_assert(!error);
    *import = (wasmtime_extern_t) {.kind = WASMTIME_EXTERN_GLOBAL, .of.global = global};
  } else if (name_eq(import_name, "__table_base")) {
    wasmtime_val_t value = WASM_I32_VAL(self->current_function_table_offset);
    wasmtime_global_t global;
    error = wasmtime_global_new(context, self->const_i32_type, &value, &global);
    ts_assert(!error);
    *import = (wasmtime_extern_t) {.kind = WASMTIME_EXTERN_GLOBAL, .of.global = global};
  } else if (name_eq(import_name, "__stack_pointer")) {
    *import = (wasmtime_extern_t) {.kind = WASMTIME_EXTERN_GLOBAL, .of.global = self->stack_pointer_global};
  } else if (name_eq(import_name, "__indirect_function_table")) {
    *import = (wasmtime_extern_t) {.kind = WASMTIME_EXTERN_TABLE, .of.table = self->function_table};
  } else if (name_eq(import_name, "memory")) {
    *import = (wasmtime_extern_t) {.kind = WASMTIME_EXTERN_MEMORY, .of.memory = self->memory};
  }

  // Builtin functions
  else if (name_eq(import_name, "__assert_fail")) {
    *import = get_builtin_extern(&self->builtin_fn_indices.assert_fail);
  } else if (name_eq(import_name, "__cxa_atexit")) {
    *import = get_builtin_extern(&self->builtin_fn_indices.at_exit);
  } else if (name_eq(import_name, "args_get")) {
    *import = get_builtin_extern(&self->builtin_fn_indices.args_get);
  } else if (name_eq(import_name, "args_sizes_get")) {
    *import = get_builtin_extern(&self->builtin_fn_indices.args_sizes_get);
  } else if (name_eq(import_name, "abort")) {
    *import = get_builtin_extern(&self->builtin_fn_indices.abort);
  } else if (name_eq(import_name, "proc_exit")) {
    *import = get_builtin_extern(&self->builtin_fn_indices.proc_exit);
  } else if (name_eq(import_name, "emscripten_notify_memory_growth")) {
    *import = get_builtin_extern(&self->builtin_fn_indices.notify_memory_growth);
  } else if (name_eq(import_name, "tree_sitter_debug_message")) {
    *import = get_builtin_extern(&self->builtin_fn_indices.debug_message);
  } else {
    return false;
  }

  return true;
}

static bool ts_wasm_store__call_module_initializer(
  TSWasmStore *self,
  const wasm_name_t *export_name,
  wasmtime_extern_t *export,
  wasm_trap_t **trap
) {
  if (
    name_eq(export_name, "_initialize") ||
    name_eq(export_name, "__wasm_apply_data_relocs") ||
    name_eq(export_name, "__wasm_call_ctors")
  ) {
    // fastedit patch: an export of another kind, or a call failing with an
    // error (a mistyped function, or the deadline), is reported as a trap
    // instead of reaching an assertion.
    if (export->kind != WASMTIME_EXTERN_FUNC) {
      const char *message = "initialization export is not a function";
      *trap = wasmtime_trap_new(message, strlen(message));
      return true;
    }
    wasmtime_context_t *context = wasmtime_store_context(self->store);
    wasmtime_func_t initialization_func = export->of.func;
    wasmtime_error_t *error = wasmtime_func_call(context, &initialization_func, NULL, 0, NULL, 0, trap);
    if (error) {
      wasm_message_t message;
      wasmtime_error_message(error, &message);
      wasmtime_error_delete(error);
      if (*trap) wasm_trap_delete(*trap);
      *trap = wasmtime_trap_new(message.data, message.size);
      wasm_byte_vec_delete(&message);
    }
    return true;
  } else {
    return false;
  }
}

TSWasmStore *ts_wasm_store_new(TSWasmEngine *engine, TSWasmError *wasm_error) {
  TSWasmStore *self = ts_calloc(1, sizeof(TSWasmStore));
  wasmtime_store_t *store = wasmtime_store_new(engine, self, NULL);
  wasmtime_context_t *context = wasmtime_store_context(store);
  // fastedit patch: with epoch interruption on the engine, ask this
  // thread's check every epoch (a store's deadline starts at 0, which
  // would stop the first call). Without it these have no effect.
  wasmtime_context_set_epoch_deadline(context, 1);
  wasmtime_store_epoch_deadline_callback(store, ts_wasm_store__epoch_deadline, NULL, NULL);
  wasmtime_error_t *error = NULL;
  wasm_trap_t *trap = NULL;
  wasm_message_t message = WASM_EMPTY_VEC;
  wasm_exporttype_vec_t export_types = WASM_EMPTY_VEC;
  wasm_importtype_vec_t import_types = WASM_EMPTY_VEC;
  wasmtime_extern_t *imports = NULL;
  wasmtime_module_t *stdlib_module = NULL;
  wasm_memorytype_t *memory_type = NULL;
  wasm_tabletype_t *table_type = NULL;
  wasmtime_func_t *lexer_funcs = NULL;

  // Define functions called by scanners via function pointers on the lexer.
  LexerInWasmMemory lexer = {
    .lookahead = 0,
    .result_symbol = 0,
  };
  FunctionDefinition lexer_definitions[] = {
    {
      &lexer.advance,
      callback__lexer_advance,
      wasm_functype_new_2_0(wasm_valtype_new_i32(), wasm_valtype_new_i32())
    },
    {
      &lexer.mark_end,
      callback__lexer_mark_end,
      wasm_functype_new_1_0(wasm_valtype_new_i32())
    },
    {
      &lexer.get_column,
      callback__lexer_get_column,
      wasm_functype_new_1_1(wasm_valtype_new_i32(), wasm_valtype_new_i32())
    },
    {
      &lexer.is_at_included_range_start,
      callback__lexer_is_at_included_range_start,
      wasm_functype_new_1_1(wasm_valtype_new_i32(), wasm_valtype_new_i32())
    },
    {
      &lexer.eof,
      callback__lexer_eof,
      wasm_functype_new_1_1(wasm_valtype_new_i32(), wasm_valtype_new_i32())
    },
  };

  // Define builtin functions that can be imported by scanners.
  BuiltinFunctionIndices builtin_fn_indices = {0};
  FunctionDefinition builtin_definitions[] = {
    {
      &builtin_fn_indices.proc_exit,
      callback__abort,
      wasm_functype_new_1_0(wasm_valtype_new_i32())
    },
    {
      &builtin_fn_indices.abort,
      callback__abort,
      wasm_functype_new_0_0()
    },
    {
      &builtin_fn_indices.assert_fail,
      callback__abort,
      wasm_functype_new_4_0(wasm_valtype_new_i32(), wasm_valtype_new_i32(), wasm_valtype_new_i32(), wasm_valtype_new_i32())
    },
    {
      &builtin_fn_indices.notify_memory_growth,
      callback__noop,
      wasm_functype_new_1_0(wasm_valtype_new_i32())
    },
    {
      &builtin_fn_indices.debug_message,
      callback__debug_message,
      wasm_functype_new_2_0(wasm_valtype_new_i32(), wasm_valtype_new_i32())
    },
    {
      &builtin_fn_indices.at_exit,
      callback__noop,
      wasm_functype_new_3_1(wasm_valtype_new_i32(), wasm_valtype_new_i32(), wasm_valtype_new_i32(), wasm_valtype_new_i32())
    },
    {
      &builtin_fn_indices.args_get,
      callback__noop,
      wasm_functype_new_2_1(wasm_valtype_new_i32(), wasm_valtype_new_i32(), wasm_valtype_new_i32())
    },
    {
      &builtin_fn_indices.args_sizes_get,
      callback__noop,
      wasm_functype_new_2_1(wasm_valtype_new_i32(), wasm_valtype_new_i32(), wasm_valtype_new_i32())
    },
  };

  // Create all of the Wasm functions.
  unsigned builtin_definitions_len = array_len(builtin_definitions);
  unsigned lexer_definitions_len = array_len(lexer_definitions);
  lexer_funcs = ts_calloc(lexer_definitions_len, sizeof(wasmtime_func_t));
  for (unsigned i = 0; i < builtin_definitions_len; i++) {
    FunctionDefinition *definition = &builtin_definitions[i];
    wasmtime_func_t *func = (wasmtime_func_t *)definition->storage_location;
    wasmtime_func_new_unchecked(context, definition->type, definition->callback, self, NULL, func);
    wasm_functype_delete(definition->type);
  }
  for (unsigned i = 0; i < lexer_definitions_len; i++) {
    FunctionDefinition *definition = &lexer_definitions[i];
    wasmtime_func_new_unchecked(context, definition->type, definition->callback, self, NULL, &lexer_funcs[i]);
    wasm_functype_delete(definition->type);
  }

  // Compile the stdlib module.
  error = wasmtime_module_new(engine, STDLIB_WASM, STDLIB_WASM_LEN, &stdlib_module);
  if (error) {
    wasmtime_error_message(error, &message);
    wasm_error->kind = TSWasmErrorKindCompile;
    format(
      &wasm_error->message,
      "failed to compile Wasm stdlib: %.*s",
      (int)message.size, message.data
    );
    goto error;
  }

  // Retrieve the stdlib module's imports.
  wasmtime_module_imports(stdlib_module, &import_types);

  // Find the initial number of memory pages needed by the stdlib.
  const wasm_memorytype_t *stdlib_memory_type = NULL;
  for (unsigned i = 0; i < import_types.size; i++) {
    wasm_importtype_t *import_type = import_types.data[i];
    const wasm_name_t *import_name = wasm_importtype_name(import_type);
    if (name_eq(import_name, "memory")) {
      const wasm_externtype_t *type = wasm_importtype_type(import_type);
      stdlib_memory_type = wasm_externtype_as_memorytype_const(type);
    }
  }
  if (!stdlib_memory_type) {
    wasm_error->kind = TSWasmErrorKindCompile;
    format(
      &wasm_error->message,
      "Wasm stdlib is missing the 'memory' import"
    );
    goto error;
  }

  // Initialize store's memory
  uint64_t initial_memory_pages = wasmtime_memorytype_minimum(stdlib_memory_type);
  wasm_limits_t memory_limits = {.min = initial_memory_pages, .max = MAX_MEMORY_SIZE};
  memory_type = wasm_memorytype_new(&memory_limits);
  wasmtime_memory_t memory;
  error = wasmtime_memory_new(context, memory_type, &memory);
  if (error) {
    wasmtime_error_message(error, &message);
    wasm_error->kind = TSWasmErrorKindAllocate;
    format(
      &wasm_error->message,
      "failed to allocate Wasm memory: %.*s",
      (int)message.size, message.data
    );
    goto error;
  }
  wasm_memorytype_delete(memory_type);
  memory_type = NULL;

  // Initialize store's function table
  wasm_limits_t table_limits = {.min = 1, .max = wasm_limits_max_default};
  table_type = wasm_tabletype_new(wasm_valtype_new(WASM_FUNCREF), &table_limits);
  wasmtime_val_t initializer = {.kind = WASMTIME_FUNCREF};
  wasmtime_table_t function_table;
  error = wasmtime_table_new(context, table_type, &initializer, &function_table);
  if (error) {
    wasmtime_error_message(error, &message);
    wasm_error->kind = TSWasmErrorKindAllocate;
    format(
      &wasm_error->message,
      "failed to allocate Wasm table: %.*s",
      (int)message.size, message.data
    );
    goto error;
  }
  wasm_tabletype_delete(table_type);
  table_type = NULL;

  unsigned stdlib_symbols_len = array_len(STDLIB_SYMBOLS);

  // Define globals for the stack and heap start addresses.
  wasm_globaltype_t *const_i32_type = wasm_globaltype_new(wasm_valtype_new_i32(), WASM_CONST);
  wasm_globaltype_t *var_i32_type = wasm_globaltype_new(wasm_valtype_new_i32(), WASM_VAR);

  wasmtime_val_t stack_pointer_value = WASM_I32_VAL(0);
  wasmtime_global_t stack_pointer_global;
  error = wasmtime_global_new(context, var_i32_type, &stack_pointer_value, &stack_pointer_global);
  wasm_globaltype_delete(var_i32_type);
  ts_assert(!error);

  *self = (TSWasmStore) {
    .engine = wasmtime_engine_clone(engine),
    .store = store,
    .memory = memory,
    .function_table = function_table,
    .language_instances = array_new(),
    .stdlib_fn_indices = ts_calloc(stdlib_symbols_len, sizeof(wasmtime_func_t)),
    .builtin_fn_indices = builtin_fn_indices,
    .stack_pointer_global = stack_pointer_global,
    .current_memory_offset = 0,
    .current_function_table_offset = 0,
    .const_i32_type = const_i32_type,
  };

  // Set up the imports for the stdlib module.
  imports = ts_calloc(import_types.size, sizeof(wasmtime_extern_t));
  for (unsigned i = 0; i < import_types.size; i++) {
    wasm_importtype_t *type = import_types.data[i];
    const wasm_name_t *import_name = wasm_importtype_name(type);
    if (!ts_wasm_store__provide_builtin_import(self, import_name, &imports[i])) {
      wasm_error->kind = TSWasmErrorKindInstantiate;
      format(
        &wasm_error->message,
        "unexpected import in Wasm stdlib: %.*s\n",
        (int)import_name->size, import_name->data
      );
      goto error;
    }
  }

  // Instantiate the stdlib module.
  wasmtime_instance_t instance;
  error = wasmtime_instance_new(context, stdlib_module, imports, import_types.size, &instance, &trap);
  ts_free(imports);
  imports = NULL;
  if (error) {
    wasmtime_error_message(error, &message);
    wasm_error->kind = TSWasmErrorKindInstantiate;
    format(
      &wasm_error->message,
      "failed to instantiate Wasm stdlib module: %.*s",
      (int)message.size, message.data
    );
    goto error;
  }
  if (trap) {
    wasm_trap_message(trap, &message);
    wasm_error->kind = TSWasmErrorKindInstantiate;
    format(
      &wasm_error->message,
      "trapped when instantiating Wasm stdlib module: %.*s",
      (int)message.size, message.data
    );
    goto error;
  }
  wasm_importtype_vec_delete(&import_types);

  // Process the stdlib module's exports.
  for (unsigned i = 0; i < stdlib_symbols_len; i++) {
    self->stdlib_fn_indices[i] = (wasmtime_func_t){.store_id = 0};
  }
  wasmtime_module_exports(stdlib_module, &export_types);
  for (unsigned i = 0; i < export_types.size; i++) {
    wasm_exporttype_t *export_type = export_types.data[i];
    const wasm_name_t *name = wasm_exporttype_name(export_type);

    char *export_name;
    size_t name_len;
    wasmtime_extern_t export = {.kind = WASM_EXTERN_GLOBAL};
    bool exists = wasmtime_instance_export_nth(context, &instance, i, &export_name, &name_len, &export);
    ts_assert(exists);

    if (export.kind == WASMTIME_EXTERN_GLOBAL) {
      if (name_eq(name, "__stack_pointer")) {
        self->stack_pointer_global = export.of.global;
      }
    }

    if (export.kind == WASMTIME_EXTERN_FUNC) {
      if (ts_wasm_store__call_module_initializer(self, name, &export, &trap)) {
        if (trap) {
          wasm_trap_message(trap, &message);
          wasm_error->kind = TSWasmErrorKindInstantiate;
          format(
            &wasm_error->message,
            "trap when calling stdlib relocation function: %.*s\n",
            (int)message.size, message.data
          );
          goto error;
        }
        continue;
      }

      if (name_eq(name, "reset_heap")) {
        self->builtin_fn_indices.reset_heap = export.of.func;
        continue;
      }

      for (unsigned j = 0; j < stdlib_symbols_len; j++) {
        if (name_eq(name, STDLIB_SYMBOLS[j])) {
          self->stdlib_fn_indices[j] = export.of.func;
          break;
        }
      }
    }
  }

  if (self->builtin_fn_indices.reset_heap.store_id == 0) {
    wasm_error->kind = TSWasmErrorKindInstantiate;
    format(
      &wasm_error->message,
      "missing malloc reset function in Wasm stdlib"
    );
    goto error;
  }

  for (unsigned i = 0; i < stdlib_symbols_len; i++) {
    if (self->stdlib_fn_indices[i].store_id == 0) {
      wasm_error->kind = TSWasmErrorKindInstantiate;
      format(
        &wasm_error->message,
        "missing exported symbol in Wasm stdlib: %s",
        STDLIB_SYMBOLS[i]
      );
      goto error;
    }
  }

  wasm_exporttype_vec_delete(&export_types);
  wasmtime_module_delete(stdlib_module);

  // Add all of the lexer callback functions to the function table. Store their function table
  // indices on the in-memory lexer.
  uint64_t table_index;
  error = wasmtime_table_grow(context, &function_table, lexer_definitions_len, &initializer, &table_index);
  if (error) {
    wasmtime_error_message(error, &message);
    wasm_error->kind = TSWasmErrorKindAllocate;
    format(
      &wasm_error->message,
      "failed to grow Wasm table to initial size: %.*s",
      (int)message.size, message.data
    );
    goto error;
  }
  for (unsigned i = 0; i < lexer_definitions_len; i++) {
    FunctionDefinition *definition = &lexer_definitions[i];
    wasmtime_val_t func_val = {.kind = WASMTIME_FUNCREF, .of.funcref = lexer_funcs[i]};
    error = wasmtime_table_set(context, &function_table, table_index, &func_val);
    ts_assert(!error);
    *(int32_t *)(definition->storage_location) = table_index;
    table_index++;
  }
  ts_free(lexer_funcs);

  self->current_function_table_offset = table_index;
  self->lexer_address = initial_memory_pages * MEMORY_PAGE_SIZE;
  self->current_memory_offset = self->lexer_address + sizeof(LexerInWasmMemory);

  // Grow the memory enough to hold the builtin lexer and serialization buffer.
  uint32_t new_pages_needed = (self->current_memory_offset - self->lexer_address - 1) / MEMORY_PAGE_SIZE + 1;
  uint64_t prev_memory_size;
  wasmtime_memory_grow(context, &memory, new_pages_needed, &prev_memory_size);

  uint8_t *memory_data = wasmtime_memory_data(context, &memory);
  memcpy(&memory_data[self->lexer_address], &lexer, sizeof(lexer));
  wasmtime_global_get(context, &self->stack_pointer_global, &self->stack_pointer_start);
  return self;

error:
  ts_free(self);
  if (stdlib_module) wasmtime_module_delete(stdlib_module);
  if (store) wasmtime_store_delete(store);
  if (import_types.size) wasm_importtype_vec_delete(&import_types);
  if (memory_type) wasm_memorytype_delete(memory_type);
  if (table_type) wasm_tabletype_delete(table_type);
  if (trap) wasm_trap_delete(trap);
  if (error) wasmtime_error_delete(error);
  if (message.size) wasm_byte_vec_delete(&message);
  if (export_types.size) wasm_exporttype_vec_delete(&export_types);
  if (imports) ts_free(imports);
  ts_free(lexer_funcs);
  return NULL;
}

void ts_wasm_store_delete(TSWasmStore *self) {
  if (!self) return;
  ts_free(self->stdlib_fn_indices);
  wasm_globaltype_delete(self->const_i32_type);
  wasmtime_store_delete(self->store);
  wasm_engine_delete(self->engine);
  for (unsigned i = 0; i < self->language_instances.size; i++) {
    LanguageWasmInstance *instance = array_get(&self->language_instances, i);
    language_id_delete(instance->language_id);
  }
  array_delete(&self->language_instances);
  ts_free(self);
}

size_t ts_wasm_store_language_count(const TSWasmStore *self) {
  size_t result = 0;
  for (unsigned i = 0; i < self->language_instances.size; i++) {
    const WasmLanguageId *id = array_get(&self->language_instances, i)->language_id;
    if (!id->is_language_deleted) {
      result++;
    }
  }
  return result;
}

static uint32_t ts_wasm_store__heap_address(TSWasmStore *self) {
  return self->current_memory_offset + TREE_SITTER_SERIALIZATION_BUFFER_SIZE;
}

static uint32_t ts_wasm_store__serialization_buffer_address(TSWasmStore *self) {
  return self->current_memory_offset;
}

// fastedit patch: the module's function at table `index`, which must take
// `param_count` i32s and return `result_count`, into `func`. Index 0, a null
// function pointer, leaves `func` null. False, instead of an assertion or a
// later unchecked call of the wrong type, when there's no such function.
static bool ts_wasm_store__get_typed_function(
  TSWasmStore *self,
  int32_t index,
  size_t param_count,
  size_t result_count,
  wasmtime_func_t *func
) {
  memset(func, 0, sizeof(*func));
  if (index == 0) return true;
  wasmtime_context_t *context = wasmtime_store_context(self->store);
  wasmtime_val_t value;
  if (!wasmtime_table_get(context, &self->function_table, index, &value)) return false;
  if (value.kind != WASMTIME_FUNCREF) return false;
  if (wasmtime_funcref_is_null(&value.of.funcref)) return false;
  wasm_functype_t *type = wasmtime_func_type(context, &value.of.funcref);
  const wasm_valtype_vec_t *params = wasm_functype_params(type);
  const wasm_valtype_vec_t *results = wasm_functype_results(type);
  bool matches = params->size == param_count && results->size == result_count;
  for (size_t i = 0; matches && i < params->size; i++) {
    matches = wasm_valtype_kind(params->data[i]) == WASM_I32;
  }
  for (size_t i = 0; matches && i < results->size; i++) {
    matches = wasm_valtype_kind(results->data[i]) == WASM_I32;
  }
  wasm_functype_delete(type);
  if (matches) *func = value.of.funcref;
  return matches;
}

// fastedit patch: the lexer and scanner functions a language's instance
// calls, each of the type its caller passes. False when one isn't, or the
// main lexer is missing.
static bool ts_wasm_store__get_language_functions(
  TSWasmStore *self,
  const LanguageInWasmMemory *wasm_language,
  LanguageWasmInstance *instance
) {
  return
    wasm_language->lex_fn != 0 &&
    ts_wasm_store__get_typed_function(self, wasm_language->lex_fn, 2, 1, &instance->lex_main_fn) &&
    ts_wasm_store__get_typed_function(self, wasm_language->keyword_lex_fn, 2, 1, &instance->lex_keyword_fn) &&
    ts_wasm_store__get_typed_function(self, wasm_language->external_scanner.create, 0, 1, &instance->scanner_create_fn) &&
    ts_wasm_store__get_typed_function(self, wasm_language->external_scanner.destroy, 1, 0, &instance->scanner_destroy_fn) &&
    ts_wasm_store__get_typed_function(self, wasm_language->external_scanner.serialize, 2, 1, &instance->scanner_serialize_fn) &&
    ts_wasm_store__get_typed_function(self, wasm_language->external_scanner.deserialize, 3, 0, &instance->scanner_deserialize_fn) &&
    ts_wasm_store__get_typed_function(self, wasm_language->external_scanner.scan, 3, 1, &instance->scanner_scan_fn);
}

static bool ts_wasm_store__instantiate(
  TSWasmStore *self,
  wasmtime_module_t *module,
  const char *language_name,
  const WasmDylinkInfo *dylink_info,
  wasmtime_instance_t *result,
  int32_t *language_address,
  char **error_message
) {
  wasmtime_error_t *error = NULL;
  wasm_trap_t *trap = NULL;
  wasm_message_t message = WASM_EMPTY_VEC;
  char *language_function_name = NULL;
  wasmtime_extern_t *imports = NULL;
  wasmtime_context_t *context = wasmtime_store_context(self->store);
  uint32_t initial_memory_offset = self->current_memory_offset;
  uint32_t initial_function_table_offset = self->current_function_table_offset;

  // Grow the function table to make room for the new functions.
  wasmtime_val_t initializer = {.kind = WASMTIME_FUNCREF};
  uint64_t prev_table_size;
  error = wasmtime_table_grow(context, &self->function_table, dylink_info->table_size, &initializer, &prev_table_size);
  if (error) {
    format(error_message, "invalid function table size %u", dylink_info->table_size);
    goto error;
  }

  // Grow the memory to make room for the new data.
  uint32_t needed_memory_size = ts_wasm_store__heap_address(self) + dylink_info->memory_size;
  uint32_t current_memory_size = wasmtime_memory_data_size(context, &self->memory);
  if (needed_memory_size > current_memory_size) {
    uint32_t pages_to_grow = (
      needed_memory_size - current_memory_size + MEMORY_PAGE_SIZE - 1) /
      MEMORY_PAGE_SIZE;
    uint64_t prev_memory_size;
    error = wasmtime_memory_grow(context, &self->memory, pages_to_grow, &prev_memory_size);
    if (error) {
      format(error_message, "invalid memory size %u", dylink_info->memory_size);
      goto error;
    }
  }

  // Construct the language function name as string.
  format(&language_function_name, "tree_sitter_%s", language_name);

  // Build the imports list for the module.
  wasm_importtype_vec_t import_types = WASM_EMPTY_VEC;
  wasmtime_module_imports(module, &import_types);
  imports = ts_calloc(import_types.size, sizeof(wasmtime_extern_t));

  for (unsigned i = 0; i < import_types.size; i++) {
    const wasm_importtype_t *import_type = import_types.data[i];
    const wasm_name_t *import_name = wasm_importtype_name(import_type);
    if (import_name->size == 0) {
      format(error_message, "empty import name");
      goto error;
    }

    if (ts_wasm_store__provide_builtin_import(self, import_name, &imports[i])) {
      continue;
    }

    bool defined_in_stdlib = false;
    for (unsigned j = 0; j < array_len(STDLIB_SYMBOLS); j++) {
      if (name_eq(import_name, STDLIB_SYMBOLS[j])) {
        imports[i] = (wasmtime_extern_t) {.kind = WASMTIME_EXTERN_FUNC, .of.func = self->stdlib_fn_indices[j]};
        defined_in_stdlib = true;
        break;
      }
    }

    if (!defined_in_stdlib) {
      format(
        error_message,
        "invalid import '%.*s'\n",
        (int)import_name->size, import_name->data
      );
      goto error;
    }
  }

  wasmtime_instance_t instance;
  error = wasmtime_instance_new(context, module, imports, import_types.size, &instance, &trap);
  wasm_importtype_vec_delete(&import_types);
  ts_free(imports);
  imports = NULL;
  if (error) {
    wasmtime_error_message(error, &message);
    format(
      error_message,
      "error instantiating Wasm module: %.*s\n",
      (int)message.size, message.data
    );
    goto error;
  }
  if (trap) {
    wasm_trap_message(trap, &message);
    format(
      error_message,
      "trap when instantiating Wasm module: %.*s\n",
      (int)message.size, message.data
    );
    goto error;
  }

  self->current_memory_offset += dylink_info->memory_size;
  self->current_function_table_offset += dylink_info->table_size;

  // Process the module's exports.
  bool found_language = false;
  wasmtime_extern_t language_extern;
  wasm_exporttype_vec_t export_types = WASM_EMPTY_VEC;
  wasmtime_module_exports(module, &export_types);
  for (unsigned i = 0; i < export_types.size; i++) {
    wasm_exporttype_t *export_type = export_types.data[i];
    const wasm_name_t *name = wasm_exporttype_name(export_type);

    size_t name_len;
    char *export_name;
    wasmtime_extern_t export = {.kind = WASM_EXTERN_GLOBAL};
    bool exists = wasmtime_instance_export_nth(context, &instance, i, &export_name, &name_len, &export);
    ts_assert(exists);

    // If the module exports an initialization or data-relocation function, call it.
    if (ts_wasm_store__call_module_initializer(self, name, &export, &trap)) {
      if (trap) {
        wasm_trap_message(trap, &message);
        format(
          error_message,
          "trap when calling data relocation function: %.*s\n",
          (int)message.size, message.data
        );
        goto error;
      }
    }

    // Find the main language function for the module.
    else if (name_eq(name, language_function_name)) {
      language_extern = export;
      found_language = true;
    }
  }
  wasm_exporttype_vec_delete(&export_types);

  if (!found_language) {
    format(
      error_message,
      "module did not contain language function: %s",
      language_function_name
    );
    goto error;
  }

  // fastedit patch: a language export that isn't a function, or a call
  // failing with an error (a mistyped function, or the deadline), is an
  // error instead of reaching an assertion.
  if (language_extern.kind != WASMTIME_EXTERN_FUNC) {
    format(
      error_message,
      "language export is not a function: %s",
      language_function_name
    );
    goto error;
  }

  // Invoke the language function to get the static address of the language object.
  wasmtime_func_t language_func = language_extern.of.func;
  wasmtime_val_t language_address_val;
  error = wasmtime_func_call(context, &language_func, NULL, 0, &language_address_val, 1, &trap);
  if (error) {
    wasmtime_error_message(error, &message);
    format(
      error_message,
      "error calling language function: %s: %.*s\n",
      language_function_name, (int)message.size, message.data
    );
    goto error;
  }
  if (trap) {
    wasm_trap_message(trap, &message);
    format(
      error_message,
      "trapped when calling language function: %s: %.*s\n",
      language_function_name, (int)message.size, message.data
    );
    goto error;
  }

  if (language_address_val.kind != WASMTIME_I32) {
    format(
      error_message,
      "language function did not return an integer: %s\n",
      language_function_name
    );
    goto error;
  }

  ts_free(language_function_name);
  *result = instance;
  *language_address = language_address_val.of.i32;
  return true;

error:
  self->current_memory_offset = initial_memory_offset;
  self->current_function_table_offset = initial_function_table_offset;
  if (language_function_name) ts_free(language_function_name);
  if (message.size) wasm_byte_vec_delete(&message);
  if (error) wasmtime_error_delete(error);
  if (trap) wasm_trap_delete(trap);
  if (imports) ts_free(imports);
  return false;
}

// fastedit patch: the sizes of the tables the loader copied out of a
// module's memory, which the validation below bounds their contents by.
typedef struct {
  size_t parse_action_count;     // TSParseActionEntry elements
  size_t small_parse_table_size; // uint16_t elements
  size_t external_states_size;   // bytes of the external scanner's states
  size_t alias_map_size;         // uint16_t elements
  size_t field_map_entry_count;
  size_t supertype_slice_count;
  size_t supertype_entry_count;
} TableSizes;

// fastedit patch: checks the parse action entry at `index` in the actions
// table lies in the table and holds actions that name states, symbols and
// productions the language has. `checked` marks the entries already seen.
static const char *ts_wasm_language__check_actions(
  const TSLanguage *language,
  const TableSizes *sizes,
  uint8_t *checked,
  uint32_t symbol,
  size_t index
) {
  if (index >= sizes->parse_action_count) return "parse table: an action index outside the actions";
  // 1 once the entry is checked, 2 if it holds an accept, which only the end
  // of input can take.
  if (checked[index]) {
    return checked[index] == 2 && symbol != ts_builtin_sym_end
      ? "parse actions: an accept on a token other than the end of input"
      : NULL;
  }
  size_t count = language->parse_actions[index].entry.count;
  if (count > sizes->parse_action_count - index - 1) {
    return "parse actions: an entry runs past the end of the actions";
  }
  bool accepts = false;
  for (size_t i = 1; i <= count; i++) {
    TSParseAction action = language->parse_actions[index + i].action;
    switch (action.type) {
      case TSParseActionTypeShift:
        if (!action.shift.extra && action.shift.state >= language->state_count) {
          return "parse actions: a shift to a state the language doesn't have";
        }
        break;
      case TSParseActionTypeReduce:
        if (action.reduce.symbol >= language->symbol_count) {
          return "parse actions: a reduce to a symbol the language doesn't have";
        }
        if (action.reduce.production_id >= language->production_id_count &&
            action.reduce.production_id != 0) {
          return "parse actions: a reduce of a production the language doesn't have";
        }
        if (action.reduce.production_id != 0 &&
            action.reduce.child_count > language->max_alias_sequence_length) {
          return "parse actions: a reduce of more children than an alias sequence holds";
        }
        break;
      case TSParseActionTypeAccept:
        if (symbol != ts_builtin_sym_end) {
          return "parse actions: an accept on a token other than the end of input";
        }
        accepts = true;
        break;
      case TSParseActionTypeRecover:
        break;
      default:
        return "parse actions: an action of an unknown type";
    }
  }
  checked[index] = accepts ? 2 : 1;
  return NULL;
}

// fastedit patch: checks a parse table's value for `symbol`: a terminal's is
// an index into the actions, any other's a state.
static const char *ts_wasm_language__check_table_value(
  const TSLanguage *language,
  const TableSizes *sizes,
  uint8_t *checked,
  uint32_t symbol,
  uint16_t value
) {
  if (symbol >= language->symbol_count) return "parse table: a symbol the language doesn't have";
  // A terminal's empty cell is the action entry 0, which is checked once.
  if (value == 0) return NULL;
  if (symbol < language->token_count) {
    return ts_wasm_language__check_actions(language, sizes, checked, symbol, value);
  }
  if (value >= language->state_count) return "parse table: a state the language doesn't have";
  return NULL;
}

// fastedit patch: checks every static table of a language copied out of Wasm
// memory, so that the parser, which indexes them unchecked, stays inside
// them. The header's counts were checked before the copies were sized from
// them, and the loader sized the reserved words from the lex modes' set ids.
// Returns a description of the first problem, or NULL.
static const char *ts_wasm_language__validate(
  const TSLanguage *language,
  const TableSizes *sizes
) {
  uint32_t name_count = language->symbol_count + language->alias_count;

  // The keyword fallback sets an inline subtree's symbol to it, which holds
  // symbols below UINT8_MAX.
  if (language->keyword_capture_token >= language->token_count ||
      language->keyword_capture_token >= UINT8_MAX) {
    return "keyword capture token: not a token the language has, or one inline subtrees can hold";
  }

  if (!language->symbol_names || !language->symbol_metadata || !language->public_symbol_map ||
      !language->lex_modes || !language->parse_actions || !language->parse_table ||
      (language->state_count > language->large_state_count &&
       (!language->small_parse_table || !language->small_parse_table_map))) {
    return "tables: a missing table";
  }
  for (uint32_t i = 0; i < name_count; i++) {
    if (!language->symbol_names[i]) return "symbol names: a missing name";
  }
  for (uint32_t i = 0; i < name_count; i++) {
    if (language->public_symbol_map[i] >= name_count) {
      return "public symbol map: a symbol the language doesn't have";
    }
  }
  if (language->field_count > 0) {
    if (!language->field_names || !language->field_map_slices || !language->field_map_entries) {
      return "field map: a missing table";
    }
    for (uint32_t i = 1; i <= language->field_count; i++) {
      if (!language->field_names[i]) return "field names: a missing name";
    }
    for (uint32_t i = 0; i < language->production_id_count; i++) {
      TSMapSlice slice = language->field_map_slices[i];
      if ((size_t)slice.index + slice.length > sizes->field_map_entry_count) {
        return "field map slices: a slice outside the entries";
      }
    }
    for (size_t i = 0; i < sizes->field_map_entry_count; i++) {
      if (language->field_map_entries[i].field_id > language->field_count) {
        return "field map entries: a field the language doesn't have";
      }
    }
  }

  for (uint32_t state = 0; state < language->state_count; state++) {
    TSLexerMode mode = ts_language_lex_mode_for_state(language, state);
    if (mode.external_lex_state != 0 &&
        (language->external_token_count == 0 ||
         ((size_t)mode.external_lex_state + 1) * language->external_token_count >
           sizes->external_states_size)) {
      return "lex modes: an external lex state the language doesn't have";
    }
  }
  if (language->abi_version >= LANGUAGE_VERSION_WITH_PRIMARY_STATES) {
    if (!language->primary_state_ids) return "primary state ids: a missing table";
    for (uint32_t i = 0; i < language->state_count; i++) {
      if (language->primary_state_ids[i] >= language->state_count) {
        return "primary state ids: a state the language doesn't have";
      }
    }
  }
  for (uint32_t i = 0; i < language->external_token_count; i++) {
    if (language->external_scanner.symbol_map[i] >= language->token_count) {
      return "external scanner symbol map: a symbol that isn't a token the language has";
    }
  }

  // The alias map: symbols, alias counts and aliases, ended by a null symbol.
  if (language->alias_map) {
    size_t i = 0;
    for (;;) {
      if (i >= sizes->alias_map_size) return "alias map: not ended";
      uint16_t symbol = language->alias_map[i++];
      if (symbol == 0) break;
      if (symbol >= language->symbol_count) return "alias map: a symbol the language doesn't have";
      if (i >= sizes->alias_map_size) return "alias map: cut short";
      size_t count = language->alias_map[i++];
      if (count > sizes->alias_map_size - i) return "alias map: aliases past the end of the map";
      for (size_t j = 0; j < count; j++) {
        if (language->alias_map[i + j] >= name_count) {
          return "alias map: an alias the language doesn't have";
        }
      }
      i += count;
    }
  }
  if (language->alias_sequences) {
    size_t length = (size_t)language->production_id_count * language->max_alias_sequence_length;
    for (size_t i = 0; i < length; i++) {
      if (language->alias_sequences[i] >= name_count) {
        return "alias sequences: an alias the language doesn't have";
      }
    }
  }

  if (language->abi_version >= LANGUAGE_VERSION_WITH_RESERVED_WORDS) {
    if (language->supertype_count > 0) {
      if (!language->supertype_symbols || !language->supertype_map_slices ||
          (sizes->supertype_entry_count > 0 && !language->supertype_map_entries)) {
        return "supertypes: a missing table";
      }
      for (uint32_t i = 0; i < language->supertype_count; i++) {
        if (language->supertype_symbols[i] >= language->symbol_count) {
          return "supertype symbols: a symbol the language doesn't have";
        }
      }
      for (size_t i = 0; i < sizes->supertype_slice_count; i++) {
        TSMapSlice slice = language->supertype_map_slices[i];
        if ((size_t)slice.index + slice.length > sizes->supertype_entry_count) {
          return "supertype map slices: a slice outside the entries";
        }
      }
      for (size_t i = 0; i < sizes->supertype_entry_count; i++) {
        if (language->supertype_map_entries[i] >= name_count) {
          return "supertype map entries: a symbol or alias the language doesn't have";
        }
      }
    }
    // The subtypes of any symbol marked a supertype are looked up in the map.
    for (uint32_t symbol = 0; symbol < name_count; symbol++) {
      if (language->symbol_metadata[symbol].supertype &&
          (language->supertype_count == 0 || symbol >= sizes->supertype_slice_count)) {
        return "symbol metadata: a supertype missing from the supertype map";
      }
    }
  }

  // Reserved words are terminals, ended early by a null.
  if (language->abi_version >= LANGUAGE_VERSION_WITH_RESERVED_WORDS &&
      language->reserved_words) {
    uint32_t max_id = 0;
    for (uint32_t state = 0; state < language->state_count; state++) {
      if (language->lex_modes[state].reserved_word_set_id > max_id) {
        max_id = language->lex_modes[state].reserved_word_set_id;
      }
    }
    size_t count = ((size_t)max_id + 1) * language->max_reserved_word_set_size;
    for (size_t i = 0; i < count; i++) {
      if (language->reserved_words[i] >= language->token_count) {
        return "reserved words: a token the language doesn't have";
      }
    }
  }

  // The parse tables, and the actions they name.
  uint8_t *checked = ts_calloc(sizes->parse_action_count + 1, 1);
  const char *problem = NULL;
  // An absent terminal reads the entry 0, which holds no actions.
  if (language->parse_actions[0].entry.count != 0) {
    problem = "parse actions: a first entry that isn't empty";
  }
  if (!problem && sizes->parse_action_count == 0) problem = "parse actions: none";
  for (uint32_t state = 0; !problem && state < language->large_state_count; state++) {
    const uint16_t *row = &language->parse_table[(size_t)state * language->symbol_count];
    for (uint32_t symbol = 0; !problem && symbol < language->symbol_count; symbol++) {
      problem = ts_wasm_language__check_table_value(language, sizes, checked, symbol, row[symbol]);
    }
  }
  for (
    uint32_t state = language->large_state_count;
    !problem && state < language->state_count;
    state++
  ) {
    size_t index = language->small_parse_table_map[state - language->large_state_count];
    if (index >= sizes->small_parse_table_size) {
      problem = "small parse table map: a state's entry outside the table";
      break;
    }
    const uint16_t *data = &language->small_parse_table[index];
    size_t left = sizes->small_parse_table_size - index - 1;
    size_t group_count = *data++;
    for (size_t group = 0; !problem && group < group_count; group++) {
      if (left < 2) {
        problem = "small parse table: a group runs past the end of the table";
        break;
      }
      uint16_t value = *data++;
      size_t symbol_count = *data++;
      left -= 2;
      if (symbol_count > left) {
        problem = "small parse table: a group runs past the end of the table";
        break;
      }
      if (symbol_count == 0) {
        problem = "small parse table: a group without symbols";
        break;
      }
      for (size_t i = 0; !problem && i < symbol_count; i++) {
        problem = ts_wasm_language__check_table_value(language, sizes, checked, data[i], value);
      }
      data += symbol_count;
      left -= symbol_count;
    }
  }
  ts_free(checked);
  return problem;
}

static bool ts_wasm_store__sentinel_lex_fn(TSLexer *_lexer, TSStateId state) {
  return false;
}

const TSLanguage *ts_wasm_store_load_language(
  TSWasmStore *self,
  const char *language_name,
  const char *wasm,
  uint32_t wasm_len,
  TSWasmError *wasm_error
) {
  WasmDylinkInfo dylink_info;
  wasmtime_module_t *module = NULL;
  wasmtime_error_t *error = NULL;
  TSWasmLanguage *result = NULL;
  TSLanguage *language = NULL;
  StringData symbol_name_buffer = array_new();
  StringData field_name_buffer = array_new();
  uint32_t initial_memory_offset = self->current_memory_offset;
  uint32_t initial_function_table_offset = self->current_function_table_offset;
  wasm_error->kind = TSWasmErrorKindNone;

  if (!wasm_dylink_info__parse((const unsigned char *)wasm, wasm_len, &dylink_info)) {
    wasm_error->kind = TSWasmErrorKindParse;
    format(&wasm_error->message, "failed to parse dylink section of Wasm module");
    goto error;
  }

  // Compile the Wasm code.
  error = wasmtime_module_new(self->engine, (const uint8_t *)wasm, wasm_len, &module);
  if (error) {
    wasm_message_t message;
    wasmtime_error_message(error, &message);
    wasm_error->kind = TSWasmErrorKindCompile;
    format(&wasm_error->message, "error compiling Wasm module: %.*s", (int)message.size, message.data);
    wasm_byte_vec_delete(&message);
    goto error;
  }

  // Instantiate the module in this store.
  wasmtime_instance_t instance;
  int32_t language_address;
  if (!ts_wasm_store__instantiate(
    self,
    module,
    language_name,
    &dylink_info,
    &instance,
    &language_address,
    &wasm_error->message
  )) {
    wasm_error->kind = TSWasmErrorKindInstantiate;
    goto error;
  }

  wasmtime_context_t *context = wasmtime_store_context(self->store);
  const uint8_t *memory = wasmtime_memory_data(context, &self->memory);
  WasmMemory wasm_memory = {
    .data = memory,
    .size = wasmtime_memory_data_size(context, &self->memory),
  };
  uint32_t abi_version;
  if (!wasm_memory__read(&wasm_memory, language_address, &abi_version, sizeof(abi_version))) {
    goto invalid_language_memory;
  }
  if (
    abi_version < TREE_SITTER_MIN_COMPATIBLE_LANGUAGE_VERSION ||
    abi_version > TREE_SITTER_LANGUAGE_VERSION
  ) {
    wasm_error->kind = TSWasmErrorKindInstantiate;
    format(
      &wasm_error->message,
      "incompatible language ABI version %u; expected between %u and %u",
      abi_version,
      TREE_SITTER_MIN_COMPATIBLE_LANGUAGE_VERSION,
      TREE_SITTER_LANGUAGE_VERSION
    );
    goto error;
  }

  // Copy all of the static data out of the language object in Wasm memory,
  // constructing a native language object.
  LanguageInWasmMemory wasm_language;
  bool valid_wasm_memory = true;
  if (!wasm_memory__read(&wasm_memory, language_address, &wasm_language, sizeof(LanguageInWasmMemory))) {
    goto invalid_language_memory;
  }

  // fastedit patch: the counts everything else is sized and indexed by.
  // Symbols, states, productions and fields are 16-bit ids, with the
  // top two symbols the parser's own.
  if (
    wasm_language.symbol_count == 0 ||
    wasm_language.symbol_count > UINT16_MAX - 2 ||
    wasm_language.alias_count > UINT16_MAX - 2 - wasm_language.symbol_count ||
    wasm_language.token_count == 0 ||
    wasm_language.token_count > wasm_language.symbol_count ||
    wasm_language.external_token_count > wasm_language.token_count ||
    wasm_language.state_count < 2 ||
    wasm_language.state_count > (uint32_t)UINT16_MAX + 1 ||
    wasm_language.large_state_count > wasm_language.state_count ||
    wasm_language.production_id_count > (uint32_t)UINT16_MAX + 1 ||
    wasm_language.field_count > UINT16_MAX ||
    (wasm_language.field_count > 0 && wasm_language.production_id_count == 0) ||
    // (Before ABI 15 the supertype count isn't in the language.)
    (wasm_language.abi_version >= LANGUAGE_VERSION_WITH_RESERVED_WORDS &&
      wasm_language.supertype_count > wasm_language.symbol_count)
  ) {
    wasm_error->kind = TSWasmErrorKindInstantiate;
    format(&wasm_error->message, "invalid language tables: counts the language's tables can't have");
    goto error;
  }

  bool has_supertypes =
    wasm_language.abi_version >= LANGUAGE_VERSION_WITH_RESERVED_WORDS &&
    wasm_language.supertype_count > 0;

  int32_t addresses[] = {
    wasm_language.parse_table,
    wasm_language.small_parse_table,
    wasm_language.small_parse_table_map,
    wasm_language.parse_actions,
    wasm_language.symbol_names,
    wasm_language.field_names,
    wasm_language.field_map_slices,
    wasm_language.field_map_entries,
    wasm_language.symbol_metadata,
    wasm_language.public_symbol_map,
    wasm_language.alias_map,
    wasm_language.alias_sequences,
    wasm_language.lex_modes,
    wasm_language.lex_fn,
    wasm_language.keyword_lex_fn,
    wasm_language.primary_state_ids,
    wasm_language.name,
    wasm_language.reserved_words,
    has_supertypes ? wasm_language.supertype_symbols : 0,
    has_supertypes ? wasm_language.supertype_map_entries : 0,
    has_supertypes ? wasm_language.supertype_map_slices : 0,
    wasm_language.external_token_count > 0 ? wasm_language.external_scanner.states : 0,
    wasm_language.external_token_count > 0 ? wasm_language.external_scanner.symbol_map : 0,
    wasm_language.external_token_count > 0 ? wasm_language.external_scanner.create : 0,
    wasm_language.external_token_count > 0 ? wasm_language.external_scanner.destroy : 0,
    wasm_language.external_token_count > 0 ? wasm_language.external_scanner.scan : 0,
    wasm_language.external_token_count > 0 ? wasm_language.external_scanner.serialize : 0,
    wasm_language.external_token_count > 0 ? wasm_language.external_scanner.deserialize : 0,
    language_address,
    self->current_memory_offset,
  };
  uint32_t address_count = array_len(addresses);
  TableSizes table_sizes = {0};

  result = ts_calloc(1, sizeof(TSWasmLanguage));
  language = &result->language;
  *language = (TSLanguage) {
    .abi_version = wasm_language.abi_version,
    .symbol_count = wasm_language.symbol_count,
    .alias_count = wasm_language.alias_count,
    .token_count = wasm_language.token_count,
    .external_token_count = wasm_language.external_token_count,
    .state_count = wasm_language.state_count,
    .large_state_count = wasm_language.large_state_count,
    .production_id_count = wasm_language.production_id_count,
    .field_count = wasm_language.field_count,
    .supertype_count = wasm_language.supertype_count,
    .max_alias_sequence_length = wasm_language.max_alias_sequence_length,
    .keyword_capture_token = wasm_language.keyword_capture_token,
    .metadata = wasm_language.metadata,
    .parse_table = copy(
      &wasm_memory,
      wasm_language.parse_table,
      (size_t)wasm_language.large_state_count * wasm_language.symbol_count * sizeof(uint16_t),
      &valid_wasm_memory
    ),
    .parse_actions = copy_unsized_static_array(
      &wasm_memory,
      wasm_language.parse_actions,
      addresses,
      address_count,
      &table_sizes.parse_action_count,
      &valid_wasm_memory
    ),
    .symbol_names = copy_strings(
      &wasm_memory,
      wasm_language.symbol_names,
      wasm_language.symbol_count + wasm_language.alias_count,
      &symbol_name_buffer,
      &valid_wasm_memory
    ),
    .symbol_metadata = copy(
      &wasm_memory,
      wasm_language.symbol_metadata,
      (size_t)(wasm_language.symbol_count + wasm_language.alias_count) * sizeof(TSSymbolMetadata),
      &valid_wasm_memory
    ),
    .public_symbol_map = copy(
      &wasm_memory,
      wasm_language.public_symbol_map,
      (size_t)(wasm_language.symbol_count + wasm_language.alias_count) * sizeof(TSSymbol),
      &valid_wasm_memory
    ),
    .lex_modes = copy(
      &wasm_memory,
      wasm_language.lex_modes,
      (size_t)wasm_language.state_count *
        (wasm_language.abi_version < LANGUAGE_VERSION_WITH_RESERVED_WORDS
          ? sizeof(TSLexMode)
          : sizeof(TSLexerMode)),
      &valid_wasm_memory
    ),
  };
  if (!valid_wasm_memory) goto invalid_language_memory;

  if (language->field_count > 0 && language->production_id_count > 0) {
    language->field_map_slices = copy(
      &wasm_memory,
      wasm_language.field_map_slices,
      (size_t)wasm_language.production_id_count * sizeof(TSMapSlice),
      &valid_wasm_memory
    );
    if (!valid_wasm_memory) goto invalid_language_memory;

    // Determine the number of field map entries by finding the greatest index
    // in any of the slices.
    size_t field_map_entry_count = 0;
    for (uint32_t i = 0; i < wasm_language.production_id_count; i++) {
      TSMapSlice slice = language->field_map_slices[i];
      size_t slice_end = (size_t)slice.index + slice.length;
      if (slice_end > field_map_entry_count) {
        field_map_entry_count = slice_end;
      }
    }

    language->field_map_entries = copy(
      &wasm_memory,
      wasm_language.field_map_entries,
      field_map_entry_count * sizeof(TSFieldMapEntry),
      &valid_wasm_memory
    );
    table_sizes.field_map_entry_count = field_map_entry_count;
    language->field_names = copy_strings(
      &wasm_memory,
      wasm_language.field_names,
      wasm_language.field_count + 1,
      &field_name_buffer,
      &valid_wasm_memory
    );
    if (!valid_wasm_memory) goto invalid_language_memory;
  }

  if (has_supertypes) {
    language->supertype_symbols = copy(
      &wasm_memory,
      wasm_language.supertype_symbols,
      (size_t)wasm_language.supertype_count * sizeof(TSSymbol),
      &valid_wasm_memory
    );
    if (!valid_wasm_memory) goto invalid_language_memory;

    // Determine the number of supertype map slices by finding the greatest
    // supertype ID.
    int largest_supertype = 0;
    for (unsigned i = 0; i < language->supertype_count; i++) {
      TSSymbol supertype = language->supertype_symbols[i];
      if (supertype >= language->symbol_count) {
        wasm_error->kind = TSWasmErrorKindInstantiate;
        format(&wasm_error->message, "invalid supertype symbols: a symbol the language doesn't have");
        goto error;
      }
      if (supertype > largest_supertype) {
        largest_supertype = supertype;
      }
    }

    language->supertype_map_slices = copy(
      &wasm_memory,
      wasm_language.supertype_map_slices,
      ((size_t)largest_supertype + 1) * sizeof(TSMapSlice),
      &valid_wasm_memory
    );
    if (!valid_wasm_memory) goto invalid_language_memory;

    // fastedit patch: the slices aren't in order of their entries, so the
    // entries run to the end of the furthest slice.
    size_t supertype_map_entry_count = 0;
    for (int i = 0; i <= largest_supertype; i++) {
      TSMapSlice slice = language->supertype_map_slices[i];
      size_t slice_end = (size_t)slice.index + slice.length;
      if (slice_end > supertype_map_entry_count) supertype_map_entry_count = slice_end;
    }
    table_sizes.supertype_slice_count = (size_t)largest_supertype + 1;
    table_sizes.supertype_entry_count = supertype_map_entry_count;

    language->supertype_map_entries = copy(
      &wasm_memory,
      wasm_language.supertype_map_entries,
      supertype_map_entry_count * sizeof(TSSymbol),
      &valid_wasm_memory
    );
    if (!valid_wasm_memory) goto invalid_language_memory;
  }

  {
    // fastedit patch: the alias map is read whatever the alias sequences'
    // dimensions, so it's always loaded.
    // The alias map contains symbols, alias counts, and aliases, terminated by a null symbol.
    int32_t alias_map_size = 0;
    for (;;) {
      // fastedit patch: addresses stay in int32 range.
      if ((int64_t)wasm_language.alias_map + alias_map_size + 4 > INT32_MAX) {
        goto invalid_language_memory;
      }
      TSSymbol symbol;
      if (!wasm_memory__read(&wasm_memory, wasm_language.alias_map + alias_map_size, &symbol, sizeof(symbol))) {
        goto invalid_language_memory;
      }
      alias_map_size += sizeof(TSSymbol);
      if (symbol == 0) break;
      uint16_t value_count;
      if (!wasm_memory__read(&wasm_memory, wasm_language.alias_map + alias_map_size, &value_count, sizeof(value_count))) {
        goto invalid_language_memory;
      }
      alias_map_size += sizeof(uint16_t);
      alias_map_size += value_count * sizeof(TSSymbol);
    }
    table_sizes.alias_map_size = alias_map_size / sizeof(uint16_t);
    language->alias_map = copy(
      &wasm_memory,
      wasm_language.alias_map,
      alias_map_size,
      &valid_wasm_memory
    );
    language->alias_sequences = copy(
      &wasm_memory,
      wasm_language.alias_sequences,
      (size_t)wasm_language.production_id_count * wasm_language.max_alias_sequence_length * sizeof(TSSymbol),
      &valid_wasm_memory
    );
    if (!valid_wasm_memory) goto invalid_language_memory;
  }

  if (language->state_count > language->large_state_count) {
    uint32_t small_state_count = wasm_language.state_count - wasm_language.large_state_count;
    language->small_parse_table_map = copy(
      &wasm_memory,
      wasm_language.small_parse_table_map,
      (size_t)small_state_count * sizeof(uint32_t),
      &valid_wasm_memory
    );
    language->small_parse_table = copy_unsized_static_array(
      &wasm_memory,
      wasm_language.small_parse_table,
      addresses,
      address_count,
      &table_sizes.small_parse_table_size,
      &valid_wasm_memory
    );
    if (!valid_wasm_memory) goto invalid_language_memory;
  }

  if (language->abi_version >= LANGUAGE_VERSION_WITH_PRIMARY_STATES) {
    language->primary_state_ids = copy(
      &wasm_memory,
      wasm_language.primary_state_ids,
      (size_t)wasm_language.state_count * sizeof(TSStateId),
      &valid_wasm_memory
    );
    if (!valid_wasm_memory) goto invalid_language_memory;
  }

  if (language->abi_version >= LANGUAGE_VERSION_WITH_RESERVED_WORDS) {
    language->name = copy_string(&wasm_memory, wasm_language.name, &valid_wasm_memory);
    if (!valid_wasm_memory) goto invalid_language_memory;
    language->max_reserved_word_set_size = wasm_language.max_reserved_word_set_size;

    // Determine the number of reserved word sets by finding the maximum
    // reserved_word_set_id across all lex modes.
    uint16_t max_reserved_word_set_id = 0;
    for (uint32_t i = 0; i < wasm_language.state_count; i++) {
      uint16_t id = language->lex_modes[i].reserved_word_set_id;
      if (id > max_reserved_word_set_id) max_reserved_word_set_id = id;
    }

    if (max_reserved_word_set_id > 0 && language->max_reserved_word_set_size > 0) {
      size_t reserved_word_count =
        ((size_t)max_reserved_word_set_id + 1) * language->max_reserved_word_set_size;
      language->reserved_words = copy(
          &wasm_memory,
          wasm_language.reserved_words,
          reserved_word_count * sizeof(TSSymbol),
          &valid_wasm_memory
      );
      if (!valid_wasm_memory) goto invalid_language_memory;
    }
  }

  if (language->external_token_count > 0) {
    language->external_scanner.symbol_map = copy(
      &wasm_memory,
      wasm_language.external_scanner.symbol_map,
      (size_t)wasm_language.external_token_count * sizeof(TSSymbol),
      &valid_wasm_memory
    );
    if (!valid_wasm_memory) goto invalid_language_memory;
    language->external_scanner.states = (void *)(uintptr_t)wasm_language.external_scanner.states;
    table_sizes.external_states_size = unsized_static_array_size(
      wasm_language.external_scanner.states,
      addresses,
      address_count
    );
    // fastedit patch: the states stay in the module's memory, whatever the
    // next table's address says.
    if (!wasm_memory__contains(
      &wasm_memory,
      wasm_language.external_scanner.states,
      table_sizes.external_states_size
    )) {
      wasm_error->kind = TSWasmErrorKindInstantiate;
      format(&wasm_error->message, "invalid external scanner states: outside the module's memory");
      goto error;
    }
  }

  // fastedit patch: the parser indexes the tables unchecked, so none may
  // name anything outside the language or the tables themselves. (Sizes
  // so far are in bytes.)
  table_sizes.parse_action_count /= sizeof(TSParseActionEntry);
  table_sizes.small_parse_table_size /= sizeof(uint16_t);
  const char *invalid_table = ts_wasm_language__validate(language, &table_sizes);
  if (invalid_table) {
    wasm_error->kind = TSWasmErrorKindInstantiate;
    format(&wasm_error->message, "invalid %s", invalid_table);
    goto error;
  }

  unsigned name_len = strlen(language_name);
  char *name = ts_malloc(name_len + 1);
  memcpy(name, language_name, name_len);
  name[name_len] = '\0';

  result->ref_count = 1;
  result->language_id = language_id_new();
  result->module = module;
  result->name = name;
  result->symbol_name_buffer = symbol_name_buffer.contents;
  result->field_name_buffer = field_name_buffer.contents;
  result->dylink_info = dylink_info;

  // The lex function is not called for Wasm languages. Use a sentinel to mark
  // this language as Wasm-based.
  language->lex_fn = ts_wasm_store__sentinel_lex_fn;
  language->keyword_lex_fn = NULL;

  // Clear out any instances of languages that have been deleted.
  for (unsigned i = 0; i < self->language_instances.size; i++) {
    WasmLanguageId *id = array_get(&self->language_instances, i)->language_id;
    if (id->is_language_deleted) {
      language_id_delete(id);
      array_erase(&self->language_instances, i);
      i--;
    }
  }

  // Store this store's instance of this language module.
  LanguageWasmInstance language_instance = {
    .language_id = NULL,
    .instance = instance,
    .external_states_address = wasm_language.external_scanner.states,
  };
  if (!ts_wasm_store__get_language_functions(self, &wasm_language, &language_instance)) {
    wasm_error->kind = TSWasmErrorKindInstantiate;
    format(&wasm_error->message, "invalid or mistyped lexer or scanner function");
    goto error;
  }
  language_instance.language_id = language_id_clone(result->language_id);
  array_push(&self->language_instances, language_instance);

  return language;

invalid_language_memory:
  wasm_error->kind = TSWasmErrorKindInstantiate;
  format(&wasm_error->message, "invalid language memory address");
  goto error;

error:
  self->current_memory_offset = initial_memory_offset;
  self->current_function_table_offset = initial_function_table_offset;
  delete_partially_loaded_language(result, &symbol_name_buffer, &field_name_buffer);
  if (module) wasmtime_module_delete(module);
  return NULL;
}

bool ts_wasm_store_add_language(
  TSWasmStore *self,
  const TSLanguage *language,
  uint32_t *index
) {
  wasmtime_context_t *context = wasmtime_store_context(self->store);
  const TSWasmLanguage *language_data = ts_language__wasm_language(language);

  // Search for this store's instance of the language module. Also clear out any
  // instances of languages that have been deleted.
  bool exists = false;
  for (unsigned i = 0; i < self->language_instances.size; i++) {
    WasmLanguageId *id = array_get(&self->language_instances, i)->language_id;
    if (id->is_language_deleted) {
      language_id_delete(id);
      array_erase(&self->language_instances, i);
      i--;
    } else if (id == language_data->language_id) {
      exists = true;
      *index = i;
    }
  }

  // If the language module has not been instantiated in this store, then add
  // it to this store.
  if (!exists) {
    uint32_t initial_memory_offset = self->current_memory_offset;
    uint32_t initial_function_table_offset = self->current_function_table_offset;
    *index = self->language_instances.size;
    char *message;
    wasmtime_instance_t instance;
    int32_t language_address;
    if (!ts_wasm_store__instantiate(
      self,
      language_data->module,
      language_data->name,
      &language_data->dylink_info,
      &instance,
      &language_address,
      &message
    )) {
      ts_free(message);
      return false;
    }

    LanguageInWasmMemory wasm_language;
    const uint8_t *memory = wasmtime_memory_data(context, &self->memory);
    WasmMemory wasm_memory = {
      .data = memory,
      .size = wasmtime_memory_data_size(context, &self->memory),
    };
    if (!wasm_memory__read(&wasm_memory, language_address, &wasm_language, sizeof(LanguageInWasmMemory))) {
      self->current_memory_offset = initial_memory_offset;
      self->current_function_table_offset = initial_function_table_offset;
      return false;
    }
    LanguageWasmInstance language_instance = {
      .language_id = NULL,
      .instance = instance,
      .external_states_address = wasm_language.external_scanner.states,
    };
    if (!ts_wasm_store__get_language_functions(self, &wasm_language, &language_instance)) {
      self->current_memory_offset = initial_memory_offset;
      self->current_function_table_offset = initial_function_table_offset;
      return false;
    }
    language_instance.language_id = language_id_clone(language_data->language_id);
    array_push(&self->language_instances, language_instance);
  }

  return true;
}

// fastedit patch: puts the stack pointer back too, and returns whether the
// heap was reset. It runs Wasm code (the stdlib's), which fails once this
// thread's deadline has passed, and a failure sets `has_error` instead of
// reaching an assertion; the next start resets it again.
static bool ts_wasm_store__reset_heap(TSWasmStore *self) {
  wasmtime_context_t *context = wasmtime_store_context(self->store);
  wasmtime_func_t *func = &self->builtin_fn_indices.reset_heap;
  wasm_trap_t *trap = NULL;
  wasmtime_val_t args[1] = {
    {.of.i32 = ts_wasm_store__heap_address(self), .kind = WASMTIME_I32},
  };

  wasmtime_error_t *set_error =
    wasmtime_global_set(context, &self->stack_pointer_global, &self->stack_pointer_start);
  if (set_error) wasmtime_error_delete(set_error);
  wasmtime_error_t *error = wasmtime_func_call(context, func, args, 1, NULL, 0, &trap);
  if (error) wasmtime_error_delete(error);
  if (trap) wasm_trap_delete(trap);
  if (set_error || error || trap) {
    self->has_error = true;
    return false;
  }
  return true;
}

void ts_wasm_store_reset_heap(TSWasmStore *self) {
  ts_wasm_store__reset_heap(self);
}

bool ts_wasm_store_start(TSWasmStore *self, TSLexer *lexer, const TSLanguage *language) {
  uint32_t instance_index;
  if (!ts_wasm_store_add_language(self, language, &instance_index)) return false;
  self->current_lexer = lexer;
  self->current_instance = array_get(&self->language_instances, instance_index);
  self->current_language = language;
  self->has_error = false;
  return ts_wasm_store__reset_heap(self);
}

void ts_wasm_store_reset(TSWasmStore *self) {
  self->current_lexer = NULL;
  self->current_instance = NULL;
  self->current_language = NULL;
  self->has_error = false;
  ts_wasm_store_reset_heap(self);
}

static void ts_wasm_store__call(
  TSWasmStore *self,
  wasmtime_func_t *func,
  wasmtime_val_raw_t *args_and_results,
  size_t args_and_results_len
) {
  // fastedit patch: a function the module doesn't have (a null pointer in its
  // language) fails the call instead of reaching Wasmtime.
  if (wasmtime_funcref_is_null(func)) {
    self->has_error = true;
    return;
  }
  wasmtime_context_t *context = wasmtime_store_context(self->store);
  wasm_trap_t *trap = NULL;
  wasmtime_error_t *error = wasmtime_func_call_unchecked(context, func, args_and_results, args_and_results_len, &trap);
  if (error) {
    // wasm_message_t message;
    // wasmtime_error_message(error, &message);
    // fprintf(
    //   stderr,
    //   "error in Wasm module: %.*s\n",
    //   (int)message.size, message.data
    // );
    wasmtime_error_delete(error);
    self->has_error = true;
  } else if (trap) {
    // wasm_message_t message;
    // wasm_trap_message(trap, &message);
    // fprintf(
    //   stderr,
    //   "trap in Wasm module: %.*s\n",
    //   (int)message.size, message.data
    // );
    wasm_trap_delete(trap);
    self->has_error = true;
  }
}

// The data fields of TSLexer, without the function pointers.
//
// This portion of the struct needs to be copied in and out
// of Wasm memory before and after calling a scan function.
typedef struct {
  int32_t lookahead;
  TSSymbol result_symbol;
} TSLexerDataPrefix;

static bool ts_wasm_store__call_lex_function(TSWasmStore *self, wasmtime_func_t *func, TSStateId state) {
  wasmtime_context_t *context = wasmtime_store_context(self->store);
  uint8_t *memory_data = wasmtime_memory_data(context, &self->memory);
  memcpy(
    &memory_data[self->lexer_address],
    self->current_lexer,
    sizeof(TSLexerDataPrefix)
  );

  wasmtime_val_raw_t args[2] = {
    {.i32 = self->lexer_address},
    {.i32 = state},
  };
  ts_wasm_store__call(self, func, args, 2);
  if (self->has_error) return false;
  bool result = args[0].i32;

  // fastedit patch: the memory may have moved if the call grew it, and a
  // token found must be one of the language's: anything else would index
  // its tables out of bounds.
  memory_data = wasmtime_memory_data(context, &self->memory);
  TSLexerDataPrefix data;
  memcpy(&data, &memory_data[self->lexer_address], sizeof(data));
  if (result && data.result_symbol >= self->current_language->token_count) {
    self->has_error = true;
    return false;
  }
  memcpy(self->current_lexer, &data, sizeof(data));
  return result;
}

bool ts_wasm_store_call_lex_main(TSWasmStore *self, TSStateId state) {
  return ts_wasm_store__call_lex_function(
    self,
    &self->current_instance->lex_main_fn,
    state
  );
}

bool ts_wasm_store_call_lex_keyword(TSWasmStore *self, TSStateId state) {
  return ts_wasm_store__call_lex_function(
    self,
    &self->current_instance->lex_keyword_fn,
    state
  );
}

uint32_t ts_wasm_store_call_scanner_create(TSWasmStore *self) {
  wasmtime_val_raw_t args[1] = {{.i32 = 0}};
  ts_wasm_store__call(self, &self->current_instance->scanner_create_fn, args, 1);
  if (self->has_error) return 0;
  return args[0].i32;
}

void ts_wasm_store_call_scanner_destroy(TSWasmStore *self, uint32_t scanner_address) {
  if (self->current_instance) {
    wasmtime_val_raw_t args[1] = {{.i32 = scanner_address}};
    ts_wasm_store__call(self, &self->current_instance->scanner_destroy_fn, args, 1);
  }
}

bool ts_wasm_store_call_scanner_scan(
  TSWasmStore *self,
  uint32_t scanner_address,
  uint32_t valid_tokens_ix
) {
  wasmtime_context_t *context = wasmtime_store_context(self->store);
  uint8_t *memory_data = wasmtime_memory_data(context, &self->memory);

  memcpy(
    &memory_data[self->lexer_address],
    self->current_lexer,
    sizeof(TSLexerDataPrefix)
  );

  uint32_t valid_tokens_address =
    self->current_instance->external_states_address +
    (valid_tokens_ix * sizeof(bool));
  wasmtime_val_raw_t args[3] = {
    {.i32 = scanner_address},
    {.i32 = self->lexer_address},
    {.i32 = valid_tokens_address}
  };
  ts_wasm_store__call(self, &self->current_instance->scanner_scan_fn, args, 3);
  if (self->has_error) return false;
  bool result = args[0].i32;

  // fastedit patch: as for the lexers, with the external token found an
  // index into the language's external tokens.
  memory_data = wasmtime_memory_data(context, &self->memory);
  TSLexerDataPrefix data;
  memcpy(&data, &memory_data[self->lexer_address], sizeof(data));
  if (result && data.result_symbol >= self->current_language->external_token_count) {
    self->has_error = true;
    return false;
  }
  memcpy(self->current_lexer, &data, sizeof(data));
  return result;
}

uint32_t ts_wasm_store_call_scanner_serialize(
  TSWasmStore *self,
  uint32_t scanner_address,
  char *buffer
) {
  wasmtime_context_t *context = wasmtime_store_context(self->store);
  uint8_t *memory_data = wasmtime_memory_data(context, &self->memory);
  uint32_t serialization_buffer_address = ts_wasm_store__serialization_buffer_address(self);

  wasmtime_val_raw_t args[2] = {
    {.i32 = scanner_address},
    {.i32 = serialization_buffer_address},
  };
  ts_wasm_store__call(self, &self->current_instance->scanner_serialize_fn, args, 2);
  if (self->has_error) return 0;

  uint32_t length = args[0].i32;
  if (length > TREE_SITTER_SERIALIZATION_BUFFER_SIZE) {
      self->has_error = true;
      return 0;
  }

  if (length > 0) {
    memcpy(
      ((Lexer *)self->current_lexer)->debug_buffer,
      &memory_data[serialization_buffer_address],
      length
    );
  }
  return length;
}

void ts_wasm_store_call_scanner_deserialize(
  TSWasmStore *self,
  uint32_t scanner_address,
  const char *buffer,
  unsigned length
) {
  wasmtime_context_t *context = wasmtime_store_context(self->store);
  uint8_t *memory_data = wasmtime_memory_data(context, &self->memory);
  uint32_t serialization_buffer_address = ts_wasm_store__serialization_buffer_address(self);

  if (length > 0) {
    memcpy(
      &memory_data[serialization_buffer_address],
      buffer,
      length
    );
  }

  wasmtime_val_raw_t args[3] = {
    {.i32 = scanner_address},
    {.i32 = serialization_buffer_address},
    {.i32 = length},
  };
  ts_wasm_store__call(self, &self->current_instance->scanner_deserialize_fn, args, 3);
}

bool ts_wasm_store_has_error(const TSWasmStore *self) {
  return self->has_error;
}

bool ts_language_is_wasm(const TSLanguage *self) {
  return self->lex_fn == ts_wasm_store__sentinel_lex_fn;
}

void ts_wasm_language_retain(const TSLanguage *self) {
  TSWasmLanguage *language = ts_language__wasm_language(self);
  ts_assert(language->ref_count > 0);
  atomic_inc(&language->ref_count);
}

void ts_wasm_language_release(const TSLanguage *self) {
  TSWasmLanguage *language = ts_language__wasm_language(self);
  ts_assert(language->ref_count > 0);
  if (atomic_dec(&language->ref_count) == 0) {
    // Update the language id to reflect that the language is deleted. This allows any Wasm stores
    // that hold Wasm instances for this language to delete those instances.
    atomic_inc(&language->language_id->is_language_deleted);
    language_id_delete(language->language_id);

    ts_free((void *)language->field_name_buffer);
    ts_free((void *)language->symbol_name_buffer);
    ts_free((void *)language->name);
    wasmtime_module_delete(language->module);

    ts_free((void *)self->alias_map);
    ts_free((void *)self->alias_sequences);
    ts_free((void *)self->external_scanner.symbol_map);
    ts_free((void *)self->field_map_entries);
    ts_free((void *)self->field_map_slices);
    ts_free((void *)self->supertype_symbols);
    ts_free((void *)self->supertype_map_entries);
    ts_free((void *)self->supertype_map_slices);
    ts_free((void *)self->field_names);
    ts_free((void *)self->lex_modes);
    ts_free((void *)self->name);
    ts_free((void *)self->reserved_words);
    ts_free((void *)self->parse_actions);
    ts_free((void *)self->parse_table);
    ts_free((void *)self->primary_state_ids);
    ts_free((void *)self->public_symbol_map);
    ts_free((void *)self->small_parse_table);
    ts_free((void *)self->small_parse_table_map);
    ts_free((void *)self->symbol_metadata);
    ts_free((void *)self->symbol_names);
    ts_free((void *)self);
  }
}

#ifdef _MSC_VER
#pragma warning(pop)
#elif defined(__GNUC__) || defined(__clang__)
#pragma GCC diagnostic pop
#endif

#else

// If the Wasm feature is not enabled, define dummy versions of all of the
// Wasm-related functions.

void ts_wasm_store_delete(TSWasmStore *self) {
  (void)self;
}

bool ts_wasm_store_start(
  TSWasmStore *self,
  TSLexer *lexer,
  const TSLanguage *language
) {
  (void)self;
  (void)lexer;
  (void)language;
  return false;
}

void ts_wasm_store_reset(TSWasmStore *self) {
  (void)self;
}

bool ts_wasm_store_call_lex_main(TSWasmStore *self, TSStateId state) {
  (void)self;
  (void)state;
  return false;
}

bool ts_wasm_store_call_lex_keyword(TSWasmStore *self, TSStateId state) {
  (void)self;
  (void)state;
  return false;
}

uint32_t ts_wasm_store_call_scanner_create(TSWasmStore *self) {
  (void)self;
  return 0;
}

void ts_wasm_store_call_scanner_destroy(
  TSWasmStore *self,
  uint32_t scanner_address
) {
  (void)self;
  (void)scanner_address;
}

bool ts_wasm_store_call_scanner_scan(
  TSWasmStore *self,
  uint32_t scanner_address,
  uint32_t valid_tokens_ix
) {
  (void)self;
  (void)scanner_address;
  (void)valid_tokens_ix;
  return false;
}

uint32_t ts_wasm_store_call_scanner_serialize(
  TSWasmStore *self,
  uint32_t scanner_address,
  char *buffer
) {
  (void)self;
  (void)scanner_address;
  (void)buffer;
  return 0;
}

void ts_wasm_store_call_scanner_deserialize(
  TSWasmStore *self,
  uint32_t scanner_address,
  const char *buffer,
  unsigned length
) {
  (void)self;
  (void)scanner_address;
  (void)buffer;
  (void)length;
}

bool ts_wasm_store_has_error(const TSWasmStore *self) {
  (void)self;
  return false;
}

bool ts_language_is_wasm(const TSLanguage *self) {
  (void)self;
  return false;
}

void ts_wasm_language_retain(const TSLanguage *self) {
  (void)self;
}

void ts_wasm_language_release(const TSLanguage *self) {
  (void)self;
}

#endif
