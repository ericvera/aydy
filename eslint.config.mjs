import js from '@eslint/js'
import unocss from '@unocss/eslint-config/flat'
import { defineConfig, globalIgnores } from 'eslint/config'
import configPrettier from 'eslint-config-prettier/flat'
import pluginVue from 'eslint-plugin-vue'
import tseslint from 'typescript-eslint'

export default defineConfig(
  globalIgnores([
    '.husky',
    '.mise',
    '.yarn',
    'dist',
    'extension/out',
    'src-tauri/target',
    'src-tauri/gen',
  ]),
  js.configs.recommended,
  tseslint.configs.recommended,
  pluginVue.configs['flat/recommended'],
  unocss,
  {
    name: 'local:vue-ts',
    files: ['**/*.vue'],
    languageOptions: {
      parserOptions: {
        parser: tseslint.parser,
      },
    },
    rules: {
      // TypeScript already reports undefined identifiers, and it knows about
      // the DOM lib. typescript-eslint turns this off for .ts files but its
      // glob does not cover .vue.
      'no-undef': 'off',
    },
  },
  // Turns off every ESLint rule that fights prettier's output. Must stay
  // after the shared configs above and before the local rules below, which
  // deliberately re-enable a comment-only max-len.
  configPrettier,
  // Typed-lint rules — require parserOptions.projectService so the rule can
  // read type info. Scoped to source files; the config files below are not in
  // any tsconfig include, so the project service would throw "not found" if
  // they were caught here.
  {
    name: 'local:typed-lint',
    files: ['**/*.ts', '**/*.tsx', '**/*.mts', '**/*.cts', '**/*.vue'],
    ignores: ['vitest.config.ts', 'vite.config.ts', 'uno.config.ts'],
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
        extraFileExtensions: ['.vue'],
      },
    },
    rules: {
      '@typescript-eslint/no-unnecessary-type-assertion': 'error',
    },
  },
  {
    name: 'local:rules',
    rules: {
      curly: 'error',
      'vue/multi-word-component-names': 0,

      // Lint errors for classes in the unocss blocklist (arbitrary values and
      // tokens the strict preset does not define)
      'unocss/blocklist': 'error',

      // The class-ordering rule rewrites what prettier has already formatted
      'unocss/order': 'off',

      // Prettier enforces self-closing elements while this rule shows it as a
      // warning.
      // Fix Ref: https://github.com/vuejs/eslint-plugin-vue/issues/2232#issuecomment-1625723163
      'vue/html-self-closing': [
        'error',
        {
          html: {
            void: 'any',
          },
        },
      ],

      // Enforce comments above code, never inline
      'line-comment-position': ['error', { position: 'above' }],

      // Enforce max length for comments only
      'max-len': [
        'error',
        {
          // Effectively unlimited for code (prettier takes care of this)
          code: 999,
          // Max 80 characters for comments only
          comments: 80,
          ignoreUrls: true,
        },
      ],

      // Override default 'after-used' to catch all unused parameters
      '@typescript-eslint/no-unused-vars': [
        'error',
        {
          args: 'all',
          argsIgnorePattern: '^_',
          ignoreRestSiblings: true,
        },
      ],
    },
  },
  // Rule to block describe wrappers in test files
  {
    name: 'local:test-rules',
    files: ['**/*.test.ts', '**/*.test.js', '**/*.spec.ts', '**/*.spec.js'],
    rules: {
      // Vitest runs without globals, so the only way to write a describe
      // wrapper is to import it. This is the rule that actually fires.
      'no-restricted-imports': [
        'error',
        {
          paths: [
            {
              name: 'vitest',
              importNames: ['describe'],
              message:
                'Use flat test structure with it() methods only. Do not use describe wrappers.',
            },
          ],
        },
      ],

      // Belt and braces for a bare describe, should globals ever be enabled
      'no-restricted-globals': [
        'error',
        {
          name: 'describe',
          message:
            'Use flat test structure with it() methods only. Do not use describe wrappers.',
        },
      ],
    },
  },
)
