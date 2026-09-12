import { useOutletIdentityQuery } from "../lib/queries";

/**
 * The restaurant's name, beside the Holler mark.
 *
 * WHITE-LABEL: HOLLER IS THE PRODUCT, THE RESTAURANT IS THE BUSINESS. The name
 * comes from the `outlet` row — one seeded value, maintained by the config
 * pull — so renaming the restaurant is one change and a re-seed rather than a
 * search across four frontends. Nothing here hard-codes it, and the Holler
 * logo stays: the product mark belongs to us, the name on the door does not.
 *
 * While it loads, and if the outlet row is missing, this renders NOTHING
 * rather than a placeholder: an empty space beside the logo is honest, and
 * "Loading…" or "Unknown outlet" in a header is a worse thing to put on a
 * screen a customer can see than a moment of white space. A missing row is a
 * broken bootstrap, not a reason to stop selling.
 */
export function OutletName({ className }: { className?: string }) {
  const outlet = useOutletIdentityQuery();
  const name = outlet.data?.name ?? null;
  if (name === null || name === "") return null;
  return <span className={className ?? "outlet-name"}>{name}</span>;
}
