import React from 'react';
import { Modal, Pressable, ScrollView, Text, View } from 'react-native';
import { Button, Note, styles } from './ui';

export interface MenuAction {
  title: string;
  danger?: boolean;
  action: () => void;
}
export interface MenuConfig {
  title: string;
  detail?: string;
  actions: MenuAction[];
}
export function ActionMenu({ menu, close }: { menu: MenuConfig; close: () => void }) {
  return (
    <Modal transparent visible animationType="fade" onRequestClose={close}>
      <Pressable
        accessibilityLabel="Close menu"
        onPress={close}
        style={{ flex: 1, justifyContent: 'center', padding: 22, backgroundColor: '#000a' }}
      >
        <Pressable accessibilityViewIsModal onPress={() => undefined} style={styles.card}>
          <Text style={styles.label}>{menu.title}</Text>
          {menu.detail && <Note>{menu.detail}</Note>}
          <ScrollView style={{ maxHeight: 360 }} contentContainerStyle={{ gap: 8 }}>
            {menu.actions.map((item) => (
              <Button
                key={item.title}
                title={item.title}
                subtle
                danger={item.danger}
                onPress={() => {
                  close();
                  item.action();
                }}
              />
            ))}
          </ScrollView>
          <View>
            <Button title="Cancel" subtle onPress={close} />
          </View>
        </Pressable>
      </Pressable>
    </Modal>
  );
}
