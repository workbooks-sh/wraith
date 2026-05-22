// Type-4 clone: Same semantics (sorting) but completely different implementation
// This uses mergesort
export function sortNumbers(nums: number[]): number[] {
  if (nums.length <= 1) return nums;

  const middle = Math.floor(nums.length / 2);
  const leftHalf = nums.slice(0, middle);
  const rightHalf = nums.slice(middle);

  return merge(sortNumbers(leftHalf), sortNumbers(rightHalf));
}

function merge(left: number[], right: number[]): number[] {
  const result: number[] = [];
  let i = 0;
  let j = 0;

  while (i < left.length && j < right.length) {
    if (left[i] <= right[j]) {
      result.push(left[i]);
      i++;
    } else {
      result.push(right[j]);
      j++;
    }
  }

  while (i < left.length) {
    result.push(left[i]);
    i++;
  }

  while (j < right.length) {
    result.push(right[j]);
    j++;
  }

  return result;
}

export function searchSorted(nums: number[], target: number): number {
  let left = 0;
  let right = nums.length - 1;

  while (left <= right) {
    const center = (left + right) >> 1;
    if (nums[center] === target) return center;
    if (nums[center] < target) left = center + 1;
    else right = center - 1;
  }

  return -1;
}

export function computeMedian(nums: number[]): number {
  const ordered = sortNumbers(nums);
  const center = Math.floor(ordered.length / 2);
  if (ordered.length % 2 === 0) {
    return (ordered[center - 1] + ordered[center]) / 2;
  }
  return ordered[center];
}
