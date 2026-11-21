<script setup lang="ts">
/**
 * The screen for a settings file AYDY could not read at all.
 *
 * Until that read works AYDY cannot tell whether its hooks are there, so
 * neither the first-run screen nor the session list is an honest thing to show.
 * Something has to be: a failure nothing renders leaves an empty popover with
 * no way out of it.
 */
import type { HookFailure } from '../../types/HookFailure'
import ActionButton from '../atoms/ActionButton.vue'
import Banner from '../molecules/Banner.vue'
import ScreenHeader from '../molecules/ScreenHeader.vue'

interface Props {
  failure: HookFailure
  busy: boolean
}

const props = defineProps<Props>()

const emit = defineEmits<{ retry: []; reveal: [] }>()
</script>

<template>
  <div class="flex flex-col">
    <!-- No gear here either: the app cannot even tell whether its hooks are
      installed, so reading the file again is the only thing to offer. -->
    <ScreenHeader title="Setup" />

    <div class="min-h-0 flex-1 overflow-y-auto">
      <div class="px-5 pb-6 pt-2 text-center">
        <p class="text-2 text-neutral-6">AYDY cannot read your settings</p>
        <p class="pt-2 text-1 text-neutral-5">
          Until it can, it cannot tell whether its hooks are installed.
        </p>
      </div>

      <Banner variant="error">
        <strong class="font-3">Nothing was modified.</strong>
        {{ props.failure.reason }}
      </Banner>

      <div class="flex flex-wrap gap-3 px-5 py-4">
        <ActionButton
          variant="primary"
          :disabled="props.busy"
          @press="emit('retry')"
        >
          Try again
        </ActionButton>
        <ActionButton variant="secondary" @press="emit('reveal')">
          Reveal file
        </ActionButton>
      </div>
    </div>
  </div>
</template>
