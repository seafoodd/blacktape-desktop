import { create } from "zustand";
import { ReactNode } from "react";

export interface ModalOptions {
  dismissible?: boolean;
}

interface ModalState {
  isOpen: boolean;
  content: ReactNode | null;
  dismissible: boolean;
  stack: { content: ReactNode; dismissible: boolean }[];
  openModal: (content: ReactNode, options?: ModalOptions) => void;
  goBack: () => void;
  closeModal: () => void;
}

export const useModalStore = create<ModalState>((set, get) => ({
  isOpen: false,
  content: null,
  dismissible: true,
  stack: [],
  openModal: (content, options) => {
    const dismissible = options?.dismissible ?? true;
    const { isOpen, content: currentContent, dismissible: currentDismissible, stack } = get();
    if (isOpen && currentContent) {
      set({
        stack: [...stack, { content: currentContent, dismissible: currentDismissible }],
        content,
        dismissible,
      });
    } else {
      set({ isOpen: true, content, dismissible, stack: [] });
    }
  },
  goBack: () => {
    const { stack } = get();
    if (stack.length === 0) return;
    const newStack = [...stack];
    const previous = newStack.pop()!;
    set({ content: previous.content, dismissible: previous.dismissible, stack: newStack });
  },
  closeModal: () => set({ isOpen: false, content: null, dismissible: true, stack: [] }),
}));
