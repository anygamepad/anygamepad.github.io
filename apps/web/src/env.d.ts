// `.html` files import as jq79 components through the `jq79/vite` plugin.
declare module "*.html" {
  import type { Component79 } from "jq79";

  const component: Component79;
  export default component;
}
