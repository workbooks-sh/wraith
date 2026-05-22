import { index, layout, route } from "@react-router/dev/routes";

export default [
  index("./marketing/home.tsx"),
  layout("./account/layout.tsx", [route("login", "./account/login.tsx")]),
];

export const unusedRouteConfigHelper = true;
