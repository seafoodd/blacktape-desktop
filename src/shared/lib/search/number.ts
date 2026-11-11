export const formatCompactNumber = (value: number): string => {
  if (!value) return "0";

  if (value < 1_000_000) {
    return new Intl.NumberFormat("en-US").format(value);
  }

  return new Intl.NumberFormat("en-US", {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(value);
};
