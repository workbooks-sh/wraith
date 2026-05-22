<script lang="ts">
import FancyButton from './FancyButton.svelte';
import { isActive } from './utils';
import { tooltip } from './utils';
import { myAttach, Counter, inTernary, inCallback, inSpread } from './utils';
import * as utils from './utils';
import { unusedImported } from './utils';
import type { Greeting } from './types';

let name = 'world';
const counter = new Counter();
const greeting: Greeting = { recipient: name };
</script>

<FancyButton />
<div {@attach myAttach}>Attached</div>
<button use:tooltip class:active={isActive}>Hover me</button>
<button onclick={() => counter.bump()}>{counter.value}</button>
<p>{isActive ? inTernary() : ''}</p>
<p>{[1, 2].map(inCallback).join(',')}</p>
<button {...{ 'data-x': inSpread() }}>Spread</button>
<h1>Hello {utils.formatName(greeting.recipient)}!</h1>
